//! Track identity: a BLAKE3 hash of the audio stream only, so editing tags never changes it.
//!
//! For each container we locate the bytes that hold audio (excluding ID3/APE/Vorbis-comment/ilst
//! tag blocks). Hashing all of it for a 200 000-track library would read every byte of a
//! multi-terabyte collection on first scan, so the identity hash covers the region's exact length
//! plus three 32 KiB windows (start, middle, end): ~100 KB of reads per file. Distinct recordings
//! never collide in practice; identical encodings collide by design (that is what duplicate
//! detection wants). A full-content hash for verification belongs to library health (M8).
//!
//! Ogg pages carry sequence numbers and CRCs that change when a comment header grows, so for Ogg
//! only page payloads are hashed.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::Result;

const WINDOW: u64 = 32 * 1024;

/// Byte range of the audio data within a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Region {
    pub start: u64,
    pub len: u64,
}

pub(crate) fn audio_hash(path: &Path, container: &str) -> Result<String> {
    let mut f = BufReader::new(File::open(path)?);
    let file_len = f.seek(SeekFrom::End(0))?;
    let region = match container {
        "flac" => flac(&mut f, file_len)?,
        "mpeg" | "ape" | "wv" => tagged_stream(&mut f, file_len)?,
        "wav" => iff(&mut f, file_len, b"data", false)?,
        "aiff" => iff(&mut f, file_len, b"SSND", true)?,
        "mp4" => mp4_mdat(&mut f, file_len)?,
        "dsf" => iff_dsf(&mut f)?,
        "dff" => dff(&mut f, file_len)?,
        _ => None,
    }
    .unwrap_or(Region {
        start: 0,
        len: file_len,
    });

    let mut hasher = blake3::Hasher::new();
    hasher.update(container.as_bytes());
    hasher.update(&region.len.to_le_bytes());
    let windows: Vec<u64> = if region.len <= 4 * WINDOW {
        vec![0]
    } else {
        vec![0, region.len / 2 - WINDOW / 2, region.len - WINDOW]
    };
    let window_len = if region.len <= 4 * WINDOW {
        region.len
    } else {
        WINDOW
    };
    for w in windows {
        f.seek(SeekFrom::Start(region.start + w))?;
        if container == "ogg" {
            ogg_payloads(&mut f, window_len, &mut hasher)?;
        } else {
            let mut buf = vec![0u8; window_len as usize];
            f.read_exact(&mut buf)?;
            hasher.update(&buf);
        }
    }
    Ok(hasher.finalize().to_hex()[..32].to_owned())
}

fn read_at<R: Read + Seek>(r: &mut R, pos: u64, buf: &mut [u8]) -> Result<bool> {
    r.seek(SeekFrom::Start(pos))?;
    Ok(r.read_exact(buf).is_ok())
}

fn id3v2_end<R: Read + Seek>(r: &mut R) -> Result<u64> {
    let mut h = [0u8; 10];
    if !read_at(r, 0, &mut h)? || &h[..3] != b"ID3" {
        return Ok(0);
    }
    let size = ((h[6] as u64 & 0x7f) << 21)
        | ((h[7] as u64 & 0x7f) << 14)
        | ((h[8] as u64 & 0x7f) << 7)
        | (h[9] as u64 & 0x7f);
    Ok(10 + size + if h[5] & 0x10 != 0 { 10 } else { 0 })
}

/// End of audio before trailing ID3v1 / APEv2 tags.
fn trailing_tags_start<R: Read + Seek>(r: &mut R, mut end: u64) -> Result<u64> {
    loop {
        let mut tag = [0u8; 3];
        if end >= 128 && read_at(r, end - 128, &mut tag)? && &tag == b"TAG" {
            end -= 128;
            continue;
        }
        let mut footer = [0u8; 32];
        if end >= 32 && read_at(r, end - 32, &mut footer)? && &footer[..8] == b"APETAGEX" {
            let size = u32::from_le_bytes([footer[12], footer[13], footer[14], footer[15]]) as u64;
            let has_header = footer[23] & 0x80 != 0;
            end = end.saturating_sub(size + if has_header { 32 } else { 0 });
            continue;
        }
        return Ok(end);
    }
}

fn tagged_stream<R: Read + Seek>(r: &mut R, len: u64) -> Result<Option<Region>> {
    let start = id3v2_end(r)?;
    let end = trailing_tags_start(r, len)?;
    Ok((end > start).then_some(Region {
        start,
        len: end - start,
    }))
}

fn flac<R: Read + Seek>(r: &mut R, len: u64) -> Result<Option<Region>> {
    let mut pos = id3v2_end(r)?;
    let mut magic = [0u8; 4];
    if !read_at(r, pos, &mut magic)? || &magic != b"fLaC" {
        return Ok(None);
    }
    pos += 4;
    loop {
        let mut h = [0u8; 4];
        if !read_at(r, pos, &mut h)? {
            return Ok(None);
        }
        let size = u32::from_be_bytes([0, h[1], h[2], h[3]]) as u64;
        pos += 4 + size;
        if h[0] & 0x80 != 0 {
            break;
        }
    }
    let end = trailing_tags_start(r, len)?;
    Ok((end > pos).then_some(Region {
        start: pos,
        len: end - pos,
    }))
}

fn iff<R: Read + Seek>(
    r: &mut R,
    len: u64,
    want: &[u8; 4],
    big_endian: bool,
) -> Result<Option<Region>> {
    let mut pos = 12;
    while pos + 8 <= len {
        let mut h = [0u8; 8];
        if !read_at(r, pos, &mut h)? {
            break;
        }
        let size = if big_endian {
            u32::from_be_bytes([h[4], h[5], h[6], h[7]])
        } else {
            u32::from_le_bytes([h[4], h[5], h[6], h[7]])
        } as u64;
        if &h[..4] == want {
            return Ok(Some(Region {
                start: pos + 8,
                len: size.min(len - pos - 8),
            }));
        }
        pos += 8 + size + (size & 1);
    }
    Ok(None)
}

fn iff_dsf<R: Read + Seek>(r: &mut R) -> Result<Option<Region>> {
    let mut h = [0u8; 12];
    if !read_at(r, 80, &mut h)? || &h[..4] != b"data" {
        return Ok(None);
    }
    let size = u64::from_le_bytes(h[4..12].try_into().unwrap_or_default());
    Ok(Some(Region {
        start: 92,
        len: size.saturating_sub(12),
    }))
}

fn dff<R: Read + Seek>(r: &mut R, len: u64) -> Result<Option<Region>> {
    let mut pos = 16;
    while pos + 12 <= len {
        let mut h = [0u8; 12];
        if !read_at(r, pos, &mut h)? {
            break;
        }
        let size = u64::from_be_bytes(h[4..12].try_into().unwrap_or_default());
        if &h[..4] == b"DSD " || &h[..4] == b"DST " {
            return Ok(Some(Region {
                start: pos + 12,
                len: size.min(len - pos - 12),
            }));
        }
        pos += 12 + size + (size & 1);
    }
    Ok(None)
}

fn mp4_mdat<R: Read + Seek>(r: &mut R, len: u64) -> Result<Option<Region>> {
    let mut pos = 0;
    let mut best: Option<Region> = None;
    while pos + 8 <= len {
        let mut h = [0u8; 8];
        if !read_at(r, pos, &mut h)? {
            break;
        }
        let (size, header) = match u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64 {
            0 => (len - pos, 8),
            1 => {
                let mut ext = [0u8; 8];
                r.read_exact(&mut ext)?;
                (u64::from_be_bytes(ext), 16)
            }
            n => (n, 8),
        };
        if size < header {
            break;
        }
        if &h[4..8] == b"mdat" && best.is_none_or(|b| size - header > b.len) {
            best = Some(Region {
                start: pos + header,
                len: (size - header).min(len - pos - header),
            });
        }
        pos += size;
    }
    Ok(best)
}

/// Hashes the payload bytes of Ogg pages found within `limit` bytes from the current position
/// (resynchronising on the "OggS" capture pattern), skipping header pages (granule 0).
fn ogg_payloads<R: Read + Seek>(r: &mut R, limit: u64, hasher: &mut blake3::Hasher) -> Result<()> {
    let mut window = vec![0u8; limit as usize + 65_536];
    let n = read_up_to(r, &mut window)?;
    let window = &window[..n];
    let mut p = 0;
    while p + 27 <= window.len() && p < limit as usize {
        if &window[p..p + 4] != b"OggS" {
            p += 1;
            continue;
        }
        let segments = window[p + 26] as usize;
        if p + 27 + segments > window.len() {
            break;
        }
        let body: usize = window[p + 27..p + 27 + segments]
            .iter()
            .map(|&s| s as usize)
            .sum();
        let start = p + 27 + segments;
        if start + body > window.len() {
            break;
        }
        let granule = u64::from_le_bytes(window[p + 6..p + 14].try_into().unwrap_or_default());
        if granule != 0 && granule != u64::MAX {
            hasher.update(&window[start..start + body]);
        }
        p = start + body;
    }
    Ok(())
}

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = r.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}
