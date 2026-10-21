//! Monkey's Audio via the pure-Rust `ape-decoder` crate (MIT/Apache).

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use ape_decoder::ApeDecoder;

use crate::{Chunk, Decoder, Error, Result, SampleFormat, StreamSpec};

pub(crate) struct Ape {
    dec: ApeDecoder<BufReader<File>>,
    spec: StreamSpec,
    bytes_per_sample: usize,
    next_frame: u32,
    skip: usize,
    out: Vec<i32>,
}

fn ape_err(e: ape_decoder::ApeError) -> Error {
    Error::Decode(format!("ape: {e}"))
}

impl Ape {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let dec = ApeDecoder::new(BufReader::new(File::open(path)?)).map_err(ape_err)?;
        let info = dec.info();
        let bits = info.bits_per_sample as u8;
        let spec = StreamSpec {
            codec: "ape",
            lossless: true,
            sample_rate: info.sample_rate,
            channels: info.channels,
            format: SampleFormat::Int(bits),
            total_frames: Some(info.total_samples),
        };
        Ok(Self {
            bytes_per_sample: (bits as usize).div_ceil(8),
            dec,
            spec,
            next_frame: 0,
            skip: 0,
            out: Vec::new(),
        })
    }
}

impl Decoder for Ape {
    fn spec(&self) -> &StreamSpec {
        &self.spec
    }

    fn next_chunk(&mut self) -> Result<Option<Chunk<'_>>> {
        loop {
            if self.next_frame >= self.dec.total_frames() {
                return Ok(None);
            }
            let pcm = self.dec.decode_frame(self.next_frame).map_err(ape_err)?;
            self.next_frame += 1;
            self.out.clear();
            let bps = self.bytes_per_sample;
            for s in pcm.chunks_exact(bps) {
                self.out.push(match bps {
                    1 => s[0] as i32 - 128, // 8-bit WAV PCM is unsigned
                    2 => i16::from_le_bytes([s[0], s[1]]) as i32,
                    3 => i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8,
                    _ => i32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                });
            }
            let ch = self.spec.channels.max(1) as usize;
            let drop = (self.skip * ch).min(self.out.len());
            self.out.drain(..drop);
            self.skip -= drop / ch;
            if !self.out.is_empty() {
                return Ok(Some(Chunk::Int(&self.out)));
            }
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        let r = self.dec.seek(frame).map_err(ape_err)?;
        self.next_frame = r.frame_index;
        self.skip = r.skip_samples as usize;
        Ok(r.actual_sample)
    }
}
