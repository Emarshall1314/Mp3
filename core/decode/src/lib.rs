//! Decoders for every format Cadenza plays, behind one trait.
//!
//! Integer PCM comes out at its native range (a 24-bit sample is in −2²³..2²³), float PCM as
//! `f32`, DSD as bytes. Nothing is scaled, dithered or resampled here, so a lossless stream
//! decodes to exactly the samples that were encoded. Encoder delay and padding are removed, so
//! consecutive tracks join sample-accurately.
//!
//! Backends: Symphonia (FLAC, ALAC, WAV, AIFF, CAF, MP3, AAC, Vorbis, plus Ogg framing for Opus),
//! libopus, libwavpack, `ape-decoder` (Monkey's Audio), and our own DSF/DFF readers.

mod ape;
mod bounded;
mod dsd;
pub mod dsd2pcm;
mod mp4gapless;
mod sym;
mod wavpack;

use std::fs::File;
use std::io::Read;
use std::path::Path;

pub use bounded::Bounded;
pub use dsd2pcm::DsdToPcm;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("decode error: {0}")]
    Decode(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// Signed integer PCM with this many significant bits.
    Int(u8),
    /// IEEE 32-bit float PCM.
    Float,
    /// 1-bit DSD; `sample_rate` is the DSD bit rate.
    Dsd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSpec {
    pub codec: &'static str,
    pub lossless: bool,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: SampleFormat,
    /// Playable frames (after gapless trimming), when the container says.
    pub total_frames: Option<u64>,
}

/// One decoded chunk of interleaved samples, borrowed from the decoder.
#[derive(Debug, Clone, Copy)]
pub enum Chunk<'a> {
    Int(&'a [i32]),
    Float(&'a [f32]),
    /// Byte-interleaved per channel, MSB = earliest bit.
    Dsd(&'a [u8]),
}

impl Chunk<'_> {
    /// Frames in this chunk for a stream with `channels` channels.
    pub fn frames(&self, channels: u16) -> usize {
        let ch = channels.max(1) as usize;
        match self {
            Chunk::Int(s) => s.len() / ch,
            Chunk::Float(s) => s.len() / ch,
            Chunk::Dsd(s) => s.len() / ch * 8,
        }
    }
}

pub trait Decoder: Send {
    fn spec(&self) -> &StreamSpec;
    /// The next chunk, or `None` at the end of the stream.
    fn next_chunk(&mut self) -> Result<Option<Chunk<'_>>>;
    /// Seeks so the next chunk starts exactly at `frame`; returns the frame reached.
    fn seek(&mut self, frame: u64) -> Result<u64>;
}

/// Opens a file, choosing the backend by content (not extension).
pub fn open(path: &Path) -> Result<Box<dyn Decoder>> {
    let mut magic = [0u8; 12];
    let n = File::open(path)?.read(&mut magic)?;
    let magic = &magic[..n];
    if magic.starts_with(b"DSD ") || magic.starts_with(b"FRM8") {
        return Ok(Box::new(dsd::DsdFile::open(path)?));
    }
    if magic.starts_with(b"wvpk") {
        return Ok(Box::new(wavpack::WavPack::open(path)?));
    }
    if magic.starts_with(b"MAC ") || is_ape_after_id3(path, magic)? {
        return Ok(Box::new(ape::Ape::open(path)?));
    }
    Ok(Box::new(sym::SymphoniaDecoder::open(path)?))
}

/// Monkey's Audio files sometimes start with an ID3v2 tag.
fn is_ape_after_id3(path: &Path, magic: &[u8]) -> Result<bool> {
    if !magic.starts_with(b"ID3") || magic.len() < 10 {
        return Ok(false);
    }
    let size = ((magic[6] as u64) << 21)
        | ((magic[7] as u64) << 14)
        | ((magic[8] as u64) << 7)
        | magic[9] as u64;
    let mut f = File::open(path)?;
    use std::io::{Seek, SeekFrom};
    f.seek(SeekFrom::Start(10 + size))?;
    let mut m = [0u8; 4];
    Ok(f.read(&mut m)? == 4 && &m == b"MAC ")
}

/// Reads a whole stream as integers (lossless PCM) — for tests and verification tools.
pub fn decode_all_int(dec: &mut dyn Decoder) -> Result<Vec<i32>> {
    let mut out = Vec::new();
    while let Some(chunk) = dec.next_chunk()? {
        match chunk {
            Chunk::Int(s) => out.extend_from_slice(s),
            _ => return Err(Error::Unsupported("not integer PCM".into())),
        }
    }
    Ok(out)
}
