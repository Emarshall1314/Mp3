//! DSF and DSDIFF (DFF) readers producing raw DSD in the canonical layout: byte-interleaved per
//! channel, MSB = earliest bit.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::{Chunk, Decoder, Error, Result, SampleFormat, StreamSpec};

enum Layout {
    /// DSF: per-channel blocks of `block` bytes, LSB first.
    Dsf { block: usize },
    /// DFF: already byte-interleaved and MSB first.
    Dff,
}

pub(crate) struct DsdFile {
    r: BufReader<File>,
    spec: StreamSpec,
    layout: Layout,
    data_start: u64,
    /// Bytes per channel holding real samples (excludes DSF block padding).
    bytes_per_channel: u64,
    /// Next byte index (per channel) to output.
    pos: u64,
    raw: Vec<u8>,
    out: Vec<u8>,
}

fn malformed(what: &str) -> Error {
    Error::Decode(format!("dsd: {what}"))
}

impl DsdFile {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut r = BufReader::new(File::open(path)?);
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        let (rate, channels, samples, layout, data_start) = if &magic == b"DSD " {
            let mut h = [0u8; 92];
            r.seek(SeekFrom::Start(0))?;
            r.read_exact(&mut h)
                .map_err(|_| malformed("short DSF header"))?;
            let le32 = |o: usize| u32::from_le_bytes([h[o], h[o + 1], h[o + 2], h[o + 3]]);
            let le64 = |o: usize| u64::from_le_bytes(h[o..o + 8].try_into().unwrap_or_default());
            if &h[28..32] != b"fmt " || &h[80..84] != b"data" {
                return Err(malformed("unexpected DSF chunk layout"));
            }
            if le32(28 + 32) != 1 {
                return Err(Error::Unsupported(
                    "DSF with more than 1 bit per sample".into(),
                ));
            }
            (
                le32(56),
                le32(52),
                le64(64),
                Layout::Dsf {
                    block: le32(72) as usize,
                },
                92u64,
            )
        } else {
            dff_header(&mut r)?
        };
        if channels == 0 || rate == 0 {
            return Err(malformed("zero channels or rate"));
        }
        let spec = StreamSpec {
            codec: "dsd",
            lossless: true,
            sample_rate: rate,
            channels: channels as u16,
            format: SampleFormat::Dsd,
            total_frames: Some(samples),
        };
        Ok(Self {
            r,
            spec,
            layout,
            data_start,
            bytes_per_channel: samples.div_ceil(8),
            pos: 0,
            raw: Vec::new(),
            out: Vec::new(),
        })
    }
}

/// Returns (rate, channels, samples per channel, layout, data offset).
fn dff_header(r: &mut BufReader<File>) -> Result<(u32, u32, u64, Layout, u64)> {
    let mut h = [0u8; 16];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut h)?;
    if &h[12..16] != b"DSD " {
        return Err(malformed("not a DSDIFF DSD form"));
    }
    let end = 12 + u64::from_be_bytes(h[4..12].try_into().unwrap_or_default());
    let (mut rate, mut channels) = (0u32, 0u32);
    let mut data = None;
    let mut pos = 16u64;
    while pos + 12 <= end {
        r.seek(SeekFrom::Start(pos))?;
        let mut c = [0u8; 12];
        if r.read_exact(&mut c).is_err() {
            break;
        }
        let size = u64::from_be_bytes(c[4..12].try_into().unwrap_or_default());
        match &c[..4] {
            b"PROP" => {
                let mut buf = vec![0u8; size.min(1 << 16) as usize];
                r.read_exact(&mut buf)?;
                let mut p = 4;
                while p + 12 <= buf.len() {
                    let len = u64::from_be_bytes(buf[p + 4..p + 12].try_into().unwrap_or_default())
                        as usize;
                    let body = &buf[p + 12..(p + 12 + len).min(buf.len())];
                    match &buf[p..p + 4] {
                        b"FS  " if body.len() >= 4 => {
                            rate = u32::from_be_bytes([body[0], body[1], body[2], body[3]])
                        }
                        b"CHNL" if body.len() >= 2 => {
                            channels = u16::from_be_bytes([body[0], body[1]]) as u32
                        }
                        b"CMPR" if body.get(..4) != Some(b"DSD ") => {
                            return Err(Error::Unsupported("DST-compressed DSDIFF".into()));
                        }
                        _ => {}
                    }
                    p += 12 + len + (len & 1);
                }
            }
            b"DSD " => data = Some((pos + 12, size)),
            b"DST " => return Err(Error::Unsupported("DST-compressed DSDIFF".into())),
            _ => {}
        }
        pos += 12 + size + (size & 1);
    }
    let (start, len) = data.ok_or_else(|| malformed("no DSD data chunk"))?;
    let samples = if channels > 0 {
        len / channels as u64 * 8
    } else {
        0
    };
    Ok((rate, channels, samples, Layout::Dff, start))
}

const CHUNK_BYTES: u64 = 4096;

impl Decoder for DsdFile {
    fn spec(&self) -> &StreamSpec {
        &self.spec
    }

    fn next_chunk(&mut self) -> Result<Option<Chunk<'_>>> {
        let ch = self.spec.channels as usize;
        if self.pos >= self.bytes_per_channel {
            return Ok(None);
        }
        self.out.clear();
        match self.layout {
            Layout::Dsf { block } => {
                // Read the whole block group containing `pos`, emit from `pos` to its end.
                let group = self.pos / block as u64;
                let offset = (self.pos % block as u64) as usize;
                let n = ((block - offset) as u64).min(self.bytes_per_channel - self.pos) as usize;
                self.raw.resize(block * ch, 0);
                self.r.seek(SeekFrom::Start(
                    self.data_start + group * (block * ch) as u64,
                ))?;
                self.r
                    .read_exact(&mut self.raw)
                    .map_err(|_| malformed("truncated DSF data"))?;
                for i in offset..offset + n {
                    for c in 0..ch {
                        self.out.push(self.raw[c * block + i].reverse_bits());
                    }
                }
                self.pos += n as u64;
            }
            Layout::Dff => {
                let n = CHUNK_BYTES.min(self.bytes_per_channel - self.pos) as usize;
                self.raw.resize(n * ch, 0);
                self.r
                    .seek(SeekFrom::Start(self.data_start + self.pos * ch as u64))?;
                self.r
                    .read_exact(&mut self.raw)
                    .map_err(|_| malformed("truncated DFF data"))?;
                self.out.extend_from_slice(&self.raw);
                self.pos += n as u64;
            }
        }
        Ok(Some(Chunk::Dsd(&self.out)))
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        self.pos = (frame / 8).min(self.bytes_per_channel);
        Ok(self.pos * 8)
    }
}
