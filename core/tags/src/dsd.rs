//! DSF and DSDIFF (DFF): stream facts from their headers and the ID3v2 tag they carry.
//!
//! lofty supports neither container, but both embed a plain ID3v2 tag (DSF at the metadata
//! pointer, DFF in an unofficial `ID3 ` chunk). lofty only parses ID3v2 inside a container, so
//! the tag is wrapped in a minimal in-memory AIFF (FORM/AIFF + `ID3 ` chunk) and read from there.

use std::io::{Cursor, Read, Seek, SeekFrom};

use lofty::config::ParseOptions;
use lofty::file::AudioFile;
use lofty::iff::aiff::AiffFile;

use crate::{AudioInfo, Codec, Error, FieldsBuilder, FileTags, Result, TagFormat};

pub(crate) fn read<R: Read + Seek>(r: &mut R, magic: &[u8; 4]) -> Result<FileTags> {
    let (audio, id3, diin) = if magic == b"DSD " { dsf(r)? } else { dff(r)? };
    let mut legacy = Vec::new();
    let (format, fields, pictures) = match id3 {
        Some(bytes) => match id3_via_aiff(&bytes)? {
            Some(tag) => crate::id3::read(&tag, &mut legacy),
            None => (TagFormat::None, Default::default(), Vec::new()),
        },
        None => (TagFormat::None, diin, Vec::new()),
    };
    Ok(FileTags {
        format,
        fields,
        pictures,
        legacy,
        audio,
    })
}

fn id3_via_aiff(tag: &[u8]) -> Result<Option<lofty::id3::v2::Id3v2Tag>> {
    let pad = tag.len() & 1;
    let mut buf = Vec::with_capacity(tag.len() + 20);
    buf.extend_from_slice(b"FORM");
    buf.extend_from_slice(&((4 + 8 + tag.len() + pad) as u32).to_be_bytes());
    buf.extend_from_slice(b"AIFFID3 ");
    buf.extend_from_slice(&(tag.len() as u32).to_be_bytes());
    buf.extend_from_slice(tag);
    buf.resize(buf.len() + pad, 0);
    let file = AiffFile::read_from(
        &mut Cursor::new(buf),
        ParseOptions::new().read_properties(false),
    )?;
    Ok(file.id3v2().cloned())
}

fn audio(container: &'static str, rate: u32, channels: u32, samples: u64) -> AudioInfo {
    AudioInfo {
        container,
        codec: Codec::Dsd,
        sample_rate: rate,
        bit_depth: Some(1),
        float: false,
        channels: channels as u8,
        duration_ms: if rate > 0 {
            samples * 1000 / rate as u64
        } else {
            0
        },
        bitrate_kbps: (rate as u64 * channels as u64 / 1000) as u32,
        dsd_rate: Some(rate),
    }
}

type Parsed = (AudioInfo, Option<Vec<u8>>, crate::Fields);

fn dsf<R: Read + Seek>(r: &mut R) -> Result<Parsed> {
    let mut head = [0u8; 28 + 52];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut head)
        .map_err(|_| Error::Malformed("short DSF header".into()))?;
    if &head[28..32] != b"fmt " {
        return Err(Error::Malformed("DSF fmt chunk missing".into()));
    }
    let le32 = |o: usize| u32::from_le_bytes([head[o], head[o + 1], head[o + 2], head[o + 3]]);
    let le64 = |o: usize| u64::from_le_bytes(head[o..o + 8].try_into().unwrap_or_default());
    let metadata_ptr = le64(20);
    let channels = le32(28 + 24);
    let rate = le32(28 + 28);
    let samples = le64(28 + 36);
    let mut id3 = None;
    if metadata_ptr > 0 {
        r.seek(SeekFrom::Start(metadata_ptr))?;
        let mut tag = Vec::new();
        r.read_to_end(&mut tag)?;
        if tag.starts_with(b"ID3") {
            id3 = Some(tag);
        }
    }
    Ok((
        audio("dsf", rate, channels, samples),
        id3,
        Default::default(),
    ))
}

fn dff<R: Read + Seek>(r: &mut R) -> Result<Parsed> {
    let mut head = [0u8; 16];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut head)
        .map_err(|_| Error::Malformed("short DFF header".into()))?;
    if &head[12..16] != b"DSD " {
        return Err(Error::Malformed("not a DSDIFF DSD form".into()));
    }
    let form_end = 12 + u64::from_be_bytes(head[4..12].try_into().unwrap_or_default());
    let (mut rate, mut channels, mut data_len) = (0u32, 0u32, 0u64);
    let mut id3 = None;
    let mut diin = FieldsBuilder::default();
    let mut pos = 16u64;
    while pos + 12 <= form_end {
        r.seek(SeekFrom::Start(pos))?;
        let mut ch = [0u8; 12];
        if r.read_exact(&mut ch).is_err() {
            break;
        }
        let size = u64::from_be_bytes(ch[4..12].try_into().unwrap_or_default());
        let body = pos + 12;
        match &ch[..4] {
            b"PROP" => (rate, channels) = dff_prop(r, body, size)?,
            b"DSD " => data_len = size,
            b"ID3 " => {
                let mut tag = vec![0; size as usize];
                r.read_exact(&mut tag)?;
                id3 = Some(tag);
            }
            b"DIIN" => dff_diin(r, body, size, &mut diin)?,
            _ => {}
        }
        pos = body + size + (size & 1);
    }
    if rate == 0 || channels == 0 {
        return Err(Error::Malformed("DSDIFF PROP chunk missing".into()));
    }
    let samples = data_len * 8 / channels as u64;
    Ok((audio("dff", rate, channels, samples), id3, diin.finish()))
}

fn dff_prop<R: Read + Seek>(r: &mut R, start: u64, size: u64) -> Result<(u32, u32)> {
    let mut buf = vec![0; size as usize];
    r.seek(SeekFrom::Start(start))?;
    r.read_exact(&mut buf)?;
    let (mut rate, mut channels) = (0, 0);
    let mut p = 4; // skip "SND "
    while p + 12 <= buf.len() {
        let len = u64::from_be_bytes(buf[p + 4..p + 12].try_into().unwrap_or_default()) as usize;
        let body = &buf[p + 12..(p + 12 + len).min(buf.len())];
        match &buf[p..p + 4] {
            b"FS  " if body.len() >= 4 => {
                rate = u32::from_be_bytes([body[0], body[1], body[2], body[3]])
            }
            b"CHNL" if body.len() >= 2 => channels = u16::from_be_bytes([body[0], body[1]]) as u32,
            _ => {}
        }
        p += 12 + len + (len & 1);
    }
    Ok((rate, channels))
}

/// DSDIFF "edited master information": DITI (title) and DIAR (artist), used when there is no ID3.
fn dff_diin<R: Read + Seek>(r: &mut R, start: u64, size: u64, b: &mut FieldsBuilder) -> Result<()> {
    let mut buf = vec![0; size as usize];
    r.seek(SeekFrom::Start(start))?;
    r.read_exact(&mut buf)?;
    let mut p = 0;
    while p + 12 <= buf.len() {
        let len = u64::from_be_bytes(buf[p + 4..p + 12].try_into().unwrap_or_default()) as usize;
        let body = &buf[p + 12..(p + 12 + len).min(buf.len())];
        let field = match &buf[p..p + 4] {
            b"DITI" => Some("title"),
            b"DIAR" => Some("artist"),
            _ => None,
        };
        if let (Some(field), true) = (field, body.len() >= 4) {
            let n = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
            let text = &body[4..(4 + n).min(body.len())];
            b.push(field, &String::from_utf8_lossy(text));
        }
        p += 12 + len + (len & 1);
    }
    Ok(())
}
