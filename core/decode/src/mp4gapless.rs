//! Gapless information for MP4 audio (AAC): iTunes' `iTunSMPB` atom or the track's edit list.
//!
//! Symphonia parses `elst` but does not trim with it, so without this every AAC track would
//! start with ~1024 frames of encoder priming and end with padding.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Gapless {
    /// Leading frames to drop.
    pub delay: u64,
    /// Playable frames.
    pub frames: u64,
}

struct Atom {
    kind: [u8; 4],
    body: u64,
    end: u64,
}

fn next_atom<R: Read + Seek>(r: &mut R, pos: u64, limit: u64) -> Result<Option<Atom>> {
    if pos + 8 > limit {
        return Ok(None);
    }
    r.seek(SeekFrom::Start(pos))?;
    let mut h = [0u8; 8];
    r.read_exact(&mut h)?;
    let size32 = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
    let kind = [h[4], h[5], h[6], h[7]];
    let (size, header) = match size32 {
        0 => (limit - pos, 8),
        1 => {
            let mut ext = [0u8; 8];
            r.read_exact(&mut ext)?;
            (u64::from_be_bytes(ext), 16)
        }
        n => (n, 8),
    };
    if size < header || pos + size > limit {
        return Ok(None);
    }
    Ok(Some(Atom {
        kind,
        body: pos + header,
        end: pos + size,
    }))
}

fn children<R: Read + Seek>(r: &mut R, start: u64, end: u64) -> Result<Vec<Atom>> {
    let mut out = Vec::new();
    let mut pos = start;
    while let Some(a) = next_atom(r, pos, end)? {
        pos = a.end;
        out.push(a);
    }
    Ok(out)
}

fn read_body<R: Read + Seek>(r: &mut R, a: &Atom) -> Result<Vec<u8>> {
    let len = (a.end - a.body).min(1 << 20) as usize;
    let mut buf = vec![0; len];
    r.seek(SeekFrom::Start(a.body))?;
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn be32(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as u64)
}

fn be64(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8)
        .map(|s| u64::from_be_bytes(s.try_into().unwrap_or_default()))
}

pub(crate) fn read(path: &Path) -> Result<Option<Gapless>> {
    let mut r = BufReader::new(File::open(path)?);
    let len = r.seek(SeekFrom::End(0))?;
    let top = children(&mut r, 0, len)?;
    let Some(moov) = top.iter().find(|a| &a.kind == b"moov") else {
        return Ok(None);
    };
    let moov_children = children(&mut r, moov.body, moov.end)?;

    if let Some(g) = itunsmpb(&mut r, &moov_children)? {
        return Ok(Some(g));
    }

    let movie_timescale = match moov_children.iter().find(|a| &a.kind == b"mvhd") {
        Some(mvhd) => {
            let b = read_body(&mut r, mvhd)?;
            if b.first() == Some(&1) {
                be32(&b, 20)
            } else {
                be32(&b, 12)
            }
        }
        None => None,
    };
    for trak in moov_children.iter().filter(|a| &a.kind == b"trak") {
        let tc = children(&mut r, trak.body, trak.end)?;
        let Some(mdia) = tc.iter().find(|a| &a.kind == b"mdia") else {
            continue;
        };
        let mc = children(&mut r, mdia.body, mdia.end)?;
        let is_sound = match mc.iter().find(|a| &a.kind == b"hdlr") {
            Some(h) => read_body(&mut r, h)?.get(8..12) == Some(b"soun"),
            None => false,
        };
        if !is_sound {
            continue;
        }
        let Some(edts) = tc.iter().find(|a| &a.kind == b"edts") else {
            return Ok(None);
        };
        let ec = children(&mut r, edts.body, edts.end)?;
        let Some(elst) = ec.iter().find(|a| &a.kind == b"elst") else {
            return Ok(None);
        };
        let media_timescale = match mc.iter().find(|a| &a.kind == b"mdhd") {
            Some(m) => {
                let b = read_body(&mut r, m)?;
                if b.first() == Some(&1) {
                    be32(&b, 20)
                } else {
                    be32(&b, 12)
                }
            }
            None => None,
        };
        let (Some(movie_ts), Some(media_ts)) = (movie_timescale, media_timescale) else {
            return Ok(None);
        };
        let b = read_body(&mut r, elst)?;
        let version = b.first().copied().unwrap_or(0);
        let count = be32(&b, 4).unwrap_or(0);
        let entry_len = if version == 1 { 20 } else { 12 };
        for i in 0..count as usize {
            let o = 8 + i * entry_len;
            let (seg, media_time) = if version == 1 {
                (be64(&b, o), be64(&b, o + 8).map(|v| v as i64))
            } else {
                (be32(&b, o), be32(&b, o + 4).map(|v| v as u32 as i32 as i64))
            };
            let (Some(seg), Some(mt)) = (seg, media_time) else {
                break;
            };
            if mt < 0 {
                continue; // empty edit (inserted silence) — not encoder delay
            }
            // segment_duration is in movie timescale; frames are media (sample-rate) units.
            let frames = (seg as u128 * media_ts as u128 / movie_ts.max(1) as u128) as u64;
            return Ok(Some(Gapless {
                delay: mt as u64,
                frames,
            }));
        }
        return Ok(None);
    }
    Ok(None)
}

/// iTunSMPB: " 00000000 DDDDDDDD PPPPPPPP NNNNNNNNNNNNNNNN ..." (delay, padding, frames; hex).
fn itunsmpb<R: Read + Seek>(r: &mut R, moov: &[Atom]) -> Result<Option<Gapless>> {
    let Some(udta) = moov.iter().find(|a| &a.kind == b"udta") else {
        return Ok(None);
    };
    let uc = children(r, udta.body, udta.end)?;
    let Some(meta) = uc.iter().find(|a| &a.kind == b"meta") else {
        return Ok(None);
    };
    // `meta` is a full box: 4 bytes of version/flags precede its children.
    let mc = children(r, meta.body + 4, meta.end)?;
    let Some(ilst) = mc.iter().find(|a| &a.kind == b"ilst") else {
        return Ok(None);
    };
    for item in children(r, ilst.body, ilst.end)?
        .iter()
        .filter(|a| &a.kind == b"----")
    {
        let parts = children(r, item.body, item.end)?;
        let name = match parts.iter().find(|a| &a.kind == b"name") {
            Some(n) => read_body(r, n)?,
            None => continue,
        };
        if name.get(4..) != Some(b"iTunSMPB") {
            continue;
        }
        let Some(data) = parts.iter().find(|a| &a.kind == b"data") else {
            continue;
        };
        let body = read_body(r, data)?;
        let text = String::from_utf8_lossy(body.get(8..).unwrap_or_default()).into_owned();
        let fields: Vec<&str> = text.split_whitespace().collect();
        let hex = |i: usize| fields.get(i).and_then(|f| u64::from_str_radix(f, 16).ok());
        if let (Some(delay), Some(frames)) = (hex(1), hex(3)) {
            return Ok(Some(Gapless { delay, frames }));
        }
    }
    Ok(None)
}
