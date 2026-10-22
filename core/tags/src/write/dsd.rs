//! DSF and DSDIFF carry a plain ID3v2 tag, which lofty can't write into them: the tag is edited
//! through lofty, serialised, and spliced into a new copy of the container.
//!
//! - DSF: the tag sits at the end of the file, at the header's metadata pointer.
//! - DSDIFF: the tag is an `ID3 ` chunk inside the FRM8 form; it is written as the last chunk.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use lofty::config::WriteOptions as LoftyWriteOptions;
use lofty::id3::v2::Id3v2Tag;
use lofty::tag::TagExt;

use super::Plan;
use crate::{Error, FileTags, Result, TagFormat};

pub(crate) fn write(src: &Path, dst: &Path, before: &FileTags, plan: &Plan) -> Result<()> {
    let mut input = BufReader::new(File::open(src)?);
    let len = input.seek(SeekFrom::End(0))?;
    let dsf = before.audio.container == "dsf";
    let old_tag = if dsf {
        dsf_tag(&mut input, len)?
    } else {
        dff_tag(&mut input, len)?
    };
    let mut tag = match old_tag.as_deref() {
        Some(bytes) => crate::dsd::id3_via_aiff(bytes)?.unwrap_or_default(),
        None => Id3v2Tag::new(),
    };
    let v23 = before.format == TagFormat::Id3v23;
    super::id3::apply(&mut tag, plan, v23)?;
    let mut bytes = Vec::new();
    if !tag.is_empty() {
        tag.dump_to(&mut bytes, LoftyWriteOptions::new().use_id3v23(v23))
            .map_err(|e| Error::Malformed(e.to_string()))?;
    }
    let mut out = BufWriter::new(File::create(dst)?);
    if dsf {
        write_dsf(&mut input, len, &mut out, &bytes)?;
    } else {
        write_dff(&mut input, len, &mut out, &bytes)?;
    }
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(())
}

fn le64(b: &[u8]) -> u64 {
    u64::from_le_bytes(b.try_into().unwrap_or_default())
}

fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b.try_into().unwrap_or_default())
}

fn dsf_tag<R: Read + Seek>(r: &mut R, len: u64) -> Result<Option<Vec<u8>>> {
    let mut head = [0u8; 28];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut head)?;
    let ptr = le64(&head[20..28]);
    if ptr == 0 || ptr >= len {
        return Ok(None);
    }
    r.seek(SeekFrom::Start(ptr))?;
    let mut tag = Vec::new();
    r.read_to_end(&mut tag)?;
    Ok(tag.starts_with(b"ID3").then_some(tag))
}

/// Everything before the old tag (or the whole file), then the new tag; the header's file size
/// and metadata pointer updated.
fn write_dsf<R: Read + Seek, W: Write + Seek>(
    r: &mut R,
    len: u64,
    w: &mut W,
    tag: &[u8],
) -> Result<()> {
    let mut head = [0u8; 28];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut head)?;
    let ptr = le64(&head[20..28]);
    let body_end = if ptr > 0 && ptr < len { ptr } else { len };
    r.seek(SeekFrom::Start(0))?;
    io::copy(&mut r.take(body_end), w)?;
    w.write_all(tag)?;
    let total = body_end + tag.len() as u64;
    let pointer = if tag.is_empty() { 0 } else { body_end };
    w.seek(SeekFrom::Start(12))?;
    w.write_all(&total.to_le_bytes())?;
    w.write_all(&pointer.to_le_bytes())?;
    Ok(())
}

/// The chunks of a DSDIFF form: (id, start of header, body size).
fn dff_chunks<R: Read + Seek>(r: &mut R, len: u64) -> Result<Vec<([u8; 4], u64, u64)>> {
    let mut head = [0u8; 16];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut head)?;
    if &head[..4] != b"FRM8" || &head[12..16] != b"DSD " {
        return Err(Error::Malformed("not a DSDIFF DSD form".into()));
    }
    let form_end = (12 + be64(&head[4..12])).min(len);
    let mut chunks = Vec::new();
    let mut pos = 16;
    while pos + 12 <= form_end {
        let mut ch = [0u8; 12];
        r.seek(SeekFrom::Start(pos))?;
        r.read_exact(&mut ch)?;
        let size = be64(&ch[4..12]);
        let id = [ch[0], ch[1], ch[2], ch[3]];
        chunks.push((id, pos, size.min(form_end - pos - 12)));
        pos += 12 + size + (size & 1);
    }
    Ok(chunks)
}

fn dff_tag<R: Read + Seek>(r: &mut R, len: u64) -> Result<Option<Vec<u8>>> {
    for (id, start, size) in dff_chunks(r, len)? {
        if &id == b"ID3 " {
            let mut tag = vec![0; size as usize];
            r.seek(SeekFrom::Start(start + 12))?;
            r.read_exact(&mut tag)?;
            return Ok(Some(tag));
        }
    }
    Ok(None)
}

/// The form header, every chunk except the old `ID3 ` chunk byte for byte, then the new one.
fn write_dff<R: Read + Seek, W: Write + Seek>(
    r: &mut R,
    len: u64,
    w: &mut W,
    tag: &[u8],
) -> Result<()> {
    let chunks = dff_chunks(r, len)?;
    let mut head = [0u8; 16];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut head)?;
    w.write_all(&head)?;
    let mut written = 16u64;
    for (id, start, size) in chunks {
        if &id == b"ID3 " {
            continue;
        }
        let padded = 12 + size + (size & 1);
        r.seek(SeekFrom::Start(start))?;
        let copied = io::copy(&mut r.take(padded), w)?;
        // A final odd-sized chunk may lack its pad byte.
        if copied < padded {
            w.write_all(&vec![0; (padded - copied) as usize])?;
        }
        written += padded;
    }
    if !tag.is_empty() {
        let size = tag.len() as u64;
        w.write_all(b"ID3 ")?;
        w.write_all(&size.to_be_bytes())?;
        w.write_all(tag)?;
        if size & 1 == 1 {
            w.write_all(&[0])?;
        }
        written += 12 + size + (size & 1);
    }
    w.seek(SeekFrom::Start(4))?;
    w.write_all(&(written - 12).to_be_bytes())?;
    Ok(())
}
