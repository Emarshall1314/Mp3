//! Where a file's audio is, and a hash of all of it.
//!
//! Tag writes must leave the audio untouched; [`content_hash`] covers every audio byte so a write
//! can prove it. The library's identity hash samples the same [`region`] (`cadenza-library`).

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// Byte range of the audio data within a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub len: u64,
}

/// The audio bytes of a file in `container` (as [`crate::AudioInfo::container`] names it), or
/// `None` where the format has no single audio region (Ogg) or it can't be found.
pub fn region<R: Read + Seek>(
    r: &mut R,
    container: &str,
    file_len: u64,
) -> io::Result<Option<Region>> {
    match container {
        "flac" => flac(r, file_len),
        "mpeg" | "ape" | "wv" => tagged_stream(r, file_len),
        "wav" => iff(r, file_len, b"data", false),
        "aiff" => iff(r, file_len, b"SSND", true),
        "mp4" => mp4_mdat(r, file_len),
        "dsf" => iff_dsf(r),
        "dff" => dff(r, file_len),
        _ => Ok(None),
    }
}

/// BLAKE3 of every audio byte. For Ogg, the payloads of all pages after the header pages: page
/// numbers and checksums change when the comment header is rewritten, the audio doesn't.
pub fn content_hash(path: &Path, container: &str) -> io::Result<String> {
    let mut f = BufReader::new(File::open(path)?);
    let file_len = f.seek(SeekFrom::End(0))?;
    let mut hasher = blake3::Hasher::new();
    if container == "ogg" {
        f.seek(SeekFrom::Start(0))?;
        ogg_audio_payloads(&mut f, &mut hasher)?;
    } else {
        let region = region(&mut f, container, file_len)?.unwrap_or(Region {
            start: 0,
            len: file_len,
        });
        f.seek(SeekFrom::Start(region.start))?;
        let mut take = (&mut f).take(region.len);
        io::copy(&mut take, &mut HashWriter(&mut hasher))?;
    }
    Ok(hasher.finalize().to_hex().to_string())
}

struct HashWriter<'a>(&'a mut blake3::Hasher);

impl io::Write for HashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Headers end with the last page whose granule position is 0; audio pages follow.
fn ogg_audio_payloads<R: Read>(r: &mut R, hasher: &mut blake3::Hasher) -> io::Result<()> {
    let mut pages = Vec::new();
    let mut last_header = None;
    loop {
        let mut head = [0u8; 27];
        if read_up_to(r, &mut head)? < 27 || &head[..4] != b"OggS" {
            break;
        }
        let mut lacing = vec![0u8; head[26] as usize];
        r.read_exact(&mut lacing)?;
        let body: usize = lacing.iter().map(|&s| s as usize).sum();
        let mut payload = vec![0u8; body];
        r.read_exact(&mut payload)?;
        let granule = u64::from_le_bytes(head[6..14].try_into().unwrap_or_default());
        if granule == 0 {
            last_header = Some(pages.len());
        }
        pages.push(payload);
    }
    let first_audio = last_header.map_or(0, |i| i + 1);
    for payload in &pages[first_audio..] {
        hasher.update(payload);
    }
    Ok(())
}

fn read_at<R: Read + Seek>(r: &mut R, pos: u64, buf: &mut [u8]) -> io::Result<bool> {
    r.seek(SeekFrom::Start(pos))?;
    Ok(r.read_exact(buf).is_ok())
}

fn id3v2_end<R: Read + Seek>(r: &mut R) -> io::Result<u64> {
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
fn trailing_tags_start<R: Read + Seek>(r: &mut R, mut end: u64) -> io::Result<u64> {
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

fn tagged_stream<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<Option<Region>> {
    let start = id3v2_end(r)?;
    let end = trailing_tags_start(r, len)?;
    Ok((end > start).then_some(Region {
        start,
        len: end - start,
    }))
}

fn flac<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<Option<Region>> {
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
) -> io::Result<Option<Region>> {
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

fn iff_dsf<R: Read + Seek>(r: &mut R) -> io::Result<Option<Region>> {
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

fn dff<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<Option<Region>> {
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

fn mp4_mdat<R: Read + Seek>(r: &mut R, len: u64) -> io::Result<Option<Region>> {
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

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
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
