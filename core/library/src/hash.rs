//! Track identity: a BLAKE3 hash of the audio stream only, so editing tags never changes it.
//!
//! For each container we locate the bytes that hold audio ([`cadenza_tags::audio::region`]) (excluding ID3/APE/Vorbis-comment/ilst
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

use cadenza_tags::audio::{Region, region};

use crate::Result;

const WINDOW: u64 = 32 * 1024;

pub(crate) fn audio_hash(path: &Path, container: &str) -> Result<String> {
    let mut f = BufReader::new(File::open(path)?);
    let file_len = f.seek(SeekFrom::End(0))?;
    let region = region(&mut f, container, file_len)?.unwrap_or(Region {
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
