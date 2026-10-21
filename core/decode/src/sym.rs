//! Symphonia-backed decoding: FLAC, ALAC, WAV, AIFF, CAF, MP3, AAC, Vorbis, and Opus (Symphonia's
//! Ogg demuxer feeding libopus, since Symphonia has no Opus decoder).
//!
//! Positioning works on packet timestamps: a decoded packet's output begins at
//! `pts + trim_start` in the container's raw timeline. Output starts at `want_from` in that
//! timeline, which covers encoder delay that Symphonia leaves to us (MP4 edit lists, Opus
//! pre-skip) and sample-accurate seeking with the same rule.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_OPUS,
};
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

use crate::{Chunk, Decoder, Error, Result, SampleFormat, StreamSpec};

enum Backend {
    Symphonia(Box<dyn AudioDecoder>),
    Opus { dec: opus::Decoder, gain: f32 },
}

pub(crate) struct SymphoniaDecoder {
    format: Box<dyn FormatReader>,
    backend: Backend,
    track_id: u32,
    spec: StreamSpec,
    /// Significant bits of 32-bit integer buffers (e.g. 24 for left-justified 24-bit ALAC).
    s32_bits: u8,
    ints: Vec<i32>,
    floats: Vec<f32>,
    /// Raw-timeline frames before playable frame 0 (MP4 edit list / iTunSMPB, Opus pre-skip).
    delay: u64,
    /// Drop output before this raw timestamp.
    want_from: Option<i64>,
    /// Playable frames left, when the length must be enforced by us.
    remaining: Option<u64>,
    /// A decoded packet waiting to be returned: (frames, raw start timestamp).
    pending: Option<(usize, i64)>,
    consecutive_errors: u32,
}

// Symphonia's readers/decoders are Send; opus::Decoder wraps a pointer used by one thread at a time.
unsafe impl Send for SymphoniaDecoder {}

fn sym_err(e: SymError) -> Error {
    match e {
        SymError::IoError(io) => Error::Io(io),
        SymError::Unsupported(what) => Error::Unsupported(what.to_string()),
        other => Error::Decode(other.to_string()),
    }
}

/// A file viewed from `offset` on: lets Symphonia probe audio behind an ID3v2 tag too large for
/// its own metadata reader (e.g. 24 MB of embedded artwork).
struct Offset {
    file: File,
    offset: u64,
    len: u64,
}

impl Read for Offset {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buf)
    }
}

impl Seek for Offset {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let abs = match pos {
            SeekFrom::Start(p) => self.file.seek(SeekFrom::Start(self.offset + p))?,
            SeekFrom::End(d) => self.file.seek(SeekFrom::End(d))?,
            SeekFrom::Current(d) => self.file.seek(SeekFrom::Current(d))?,
        };
        Ok(abs.saturating_sub(self.offset))
    }
}

impl MediaSource for Offset {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.len - self.offset)
    }
}

fn id3v2_len(path: &Path) -> Result<Option<u64>> {
    let mut h = [0u8; 10];
    let n = File::open(path)?.read(&mut h)?;
    if n < 10 || &h[..3] != b"ID3" {
        return Ok(None);
    }
    let size = ((h[6] as u64 & 0x7f) << 21)
        | ((h[7] as u64 & 0x7f) << 14)
        | ((h[8] as u64 & 0x7f) << 7)
        | (h[9] as u64 & 0x7f);
    let footer = if h[5] & 0x10 != 0 { 10 } else { 0 };
    Ok(Some(10 + size + footer))
}

fn probe(path: &Path) -> Result<Box<dyn FormatReader>> {
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mss = MediaSourceStream::new(Box::new(File::open(path)?), Default::default());
    match symphonia::default::get_probe().probe(
        &hint,
        mss,
        FormatOptions::default(),
        MetadataOptions::default(),
    ) {
        Ok(f) => Ok(f),
        Err(first) => {
            let Some(offset) = id3v2_len(path)? else {
                return Err(sym_err(first));
            };
            let file = File::open(path)?;
            let len = file.metadata()?.len();
            let mut src = Offset { file, offset, len };
            src.seek(SeekFrom::Start(0))?;
            let mss = MediaSourceStream::new(Box::new(src), Default::default());
            symphonia::default::get_probe()
                .probe(
                    &hint,
                    mss,
                    FormatOptions::default(),
                    MetadataOptions::default(),
                )
                .map_err(|_| sym_err(first))
        }
    }
}

/// Bit depth from an ALAC magic cookie (optionally wrapped in its 12-byte `alac` atom header).
fn alac_bits(extra: Option<&[u8]>) -> Option<u8> {
    let e = extra?;
    let cfg = if e.len() >= 36 && &e[4..8] == b"alac" {
        &e[12..]
    } else {
        e
    };
    cfg.get(5)
        .copied()
        .filter(|b| matches!(b, 16 | 20 | 24 | 32))
}

/// OpusHead: pre-skip (bytes 10..12) and output gain (Q7.8 dB, bytes 16..18).
fn opus_head(head: Option<&[u8]>) -> (u64, f32) {
    match head {
        Some(h) if h.len() >= 18 && h.starts_with(b"OpusHead") => {
            let pre_skip = u16::from_le_bytes([h[10], h[11]]) as u64;
            let q78 = i16::from_le_bytes([h[16], h[17]]) as f32;
            (pre_skip, 10f32.powf(q78 / 256.0 / 20.0))
        }
        _ => (312, 1.0),
    }
}

fn codec_name(params: &AudioCodecParameters) -> &'static str {
    use symphonia::core::codecs::audio::well_known as wk;
    match params.codec {
        c if c == wk::CODEC_ID_FLAC => "flac",
        c if c == wk::CODEC_ID_ALAC => "alac",
        c if c == wk::CODEC_ID_MP3 => "mp3",
        c if c == wk::CODEC_ID_AAC => "aac",
        c if c == wk::CODEC_ID_VORBIS => "vorbis",
        c if c == wk::CODEC_ID_OPUS => "opus",
        _ => "pcm",
    }
}

impl SymphoniaDecoder {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let format = probe(path)?;
        let track = format
            .default_track(TrackType::Audio)
            .or_else(|| format.first_track(TrackType::Audio))
            .ok_or_else(|| Error::Unsupported("no audio track".into()))?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            return Err(Error::Unsupported("no audio codec parameters".into()));
        };
        let (track_id, num_frames) = (track.id, track.num_frames);
        let codec = codec_name(&params);
        let channels = params
            .channels
            .as_ref()
            .map(|c| c.count() as u16)
            .unwrap_or(0);

        if params.codec == CODEC_ID_FLAC && params.bits_per_sample == Some(32) {
            // Symphonia decodes the 33-bit side channel of 32-bit FLAC into i32 and overflows.
            return Err(Error::Unsupported(
                "32-bit integer FLAC (decoder support pending)".into(),
            ));
        }

        let (backend, delay, total) = if params.codec == CODEC_ID_OPUS {
            let ch = match channels {
                1 => opus::Channels::Mono,
                2 => opus::Channels::Stereo,
                n => return Err(Error::Unsupported(format!("Opus with {n} channels"))),
            };
            let dec = opus::Decoder::new(48_000, ch).map_err(|e| Error::Decode(e.to_string()))?;
            let (pre_skip, gain) = opus_head(params.extra_data.as_deref());
            // Symphonia trims Opus' end padding but not the pre-skip, and counts it in num_frames.
            (
                Backend::Opus { dec, gain },
                pre_skip,
                num_frames.map(|n| n.saturating_sub(pre_skip)),
            )
        } else {
            let dec = symphonia::default::get_codecs()
                .make_audio_decoder(&params, &AudioDecoderOptions::default())
                .map_err(sym_err)?;
            let gapless = if params.codec == CODEC_ID_AAC {
                crate::mp4gapless::read(path)?
            } else {
                None
            };
            match gapless {
                Some(g) => (Backend::Symphonia(dec), g.delay, Some(g.frames)),
                None => (Backend::Symphonia(dec), 0, num_frames),
            }
        };
        let s32_bits = match params.codec {
            c if c == CODEC_ID_ALAC => alac_bits(params.extra_data.as_deref()),
            _ => params.bits_per_sample.map(|b| b as u8),
        }
        .unwrap_or(32);

        let mut dec = Self {
            format,
            backend,
            track_id,
            spec: StreamSpec {
                codec,
                lossless: matches!(codec, "flac" | "alac" | "pcm"),
                sample_rate: if codec == "opus" {
                    48_000
                } else {
                    params.sample_rate.unwrap_or(0)
                },
                channels,
                format: SampleFormat::Float,
                total_frames: total,
            },
            s32_bits,
            ints: Vec::new(),
            floats: Vec::new(),
            delay,
            want_from: Some(delay as i64),
            remaining: total.filter(|_| delay > 0 || codec == "aac"),
            pending: None,
            consecutive_errors: 0,
        };
        // Decode the first packet now: the buffer type is the only reliable source of the
        // sample format (Symphonia leaves it unset for float WAV and ALAC, for example).
        dec.pending = dec.decode_packet()?;
        Ok(dec)
    }

    /// Decodes the next packet of our track into `ints`/`floats`. Returns the frame count and
    /// the raw timestamp of its first frame, or `None` at the end of the stream.
    fn decode_packet(&mut self) -> Result<Option<(usize, i64)>> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(None);
                }
                Err(e) => return Err(sym_err(e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let trim_start = packet.trim_start.get() as usize;
            let trim_end = packet.trim_end.get() as usize;
            let untrimmed = packet.dur.get() as usize + trim_start + trim_end;
            let ch = self.spec.channels.max(1) as usize;

            let (frames, is_int) = match &mut self.backend {
                Backend::Symphonia(dec) => match dec.decode(&packet) {
                    Ok(buf) => {
                        let n = buf.frames();
                        let is_int = !matches!(
                            buf,
                            GenericAudioBufferRef::F32(_) | GenericAudioBufferRef::F64(_)
                        );
                        if is_int {
                            let bits = copy_ints(&buf, &mut self.ints, self.s32_bits);
                            self.spec.format = SampleFormat::Int(bits);
                        } else {
                            self.floats.clear();
                            buf.copy_to_vec_interleaved::<f32>(&mut self.floats);
                            self.spec.format = SampleFormat::Float;
                        }
                        (n, is_int)
                    }
                    Err(SymError::DecodeError(_)) => {
                        // A damaged packet: skip it rather than ending the track.
                        self.consecutive_errors += 1;
                        if self.consecutive_errors > 32 {
                            return Err(Error::Decode(
                                "too many consecutive damaged packets".into(),
                            ));
                        }
                        continue;
                    }
                    Err(e) => return Err(sym_err(e)),
                },
                Backend::Opus { dec, gain } => {
                    self.floats.resize(5760 * ch, 0.0);
                    let n = dec
                        .decode_float(&packet.data, &mut self.floats, false)
                        .map_err(|e| Error::Decode(e.to_string()))?;
                    self.floats.truncate(n * ch);
                    if *gain != 1.0 {
                        self.floats.iter_mut().for_each(|s| *s *= *gain);
                    }
                    (n, false)
                }
            };
            self.consecutive_errors = 0;
            let start = packet.pts.get() + trim_start as i64;

            // Decoders that ignore the packet's trim return the untrimmed count; trim here.
            if frames == untrimmed && (trim_start > 0 || trim_end > 0) {
                let buf_len = if is_int {
                    self.ints.len()
                } else {
                    self.floats.len()
                };
                let keep = (trim_start * ch).min(buf_len)..((frames - trim_end) * ch).min(buf_len);
                if is_int {
                    self.ints.truncate(keep.end);
                    self.ints.drain(..keep.start);
                } else {
                    self.floats.truncate(keep.end);
                    self.floats.drain(..keep.start);
                }
                return Ok(Some((frames - trim_start - trim_end, start)));
            }
            return Ok(Some((frames, start)));
        }
    }
}

/// Copies to native-range i32; returns the significant bits.
fn copy_ints(buf: &GenericAudioBufferRef<'_>, out: &mut Vec<i32>, s32_bits: u8) -> u8 {
    use GenericAudioBufferRef as B;
    let bits = match buf {
        B::U8(_) | B::S8(_) => 8,
        B::U16(_) | B::S16(_) => 16,
        B::U24(_) | B::S24(_) => 24,
        _ => s32_bits,
    };
    out.clear();
    // Symphonia converts integers to full-scale i32 by left-shifting; undo that exactly.
    buf.copy_to_vec_interleaved::<i32>(out);
    let shift = 32 - bits as u32;
    if shift > 0 {
        out.iter_mut().for_each(|s| *s >>= shift);
    }
    bits
}

impl Decoder for SymphoniaDecoder {
    fn spec(&self) -> &StreamSpec {
        &self.spec
    }

    fn next_chunk(&mut self) -> Result<Option<Chunk<'_>>> {
        let ch = self.spec.channels.max(1) as usize;
        loop {
            if self.remaining == Some(0) {
                return Ok(None);
            }
            let next = match self.pending.take() {
                Some(p) => Some(p),
                None => self.decode_packet()?,
            };
            let Some((mut frames, start)) = next else {
                return Ok(None);
            };
            let mut offset = 0usize;
            if let Some(from) = self.want_from {
                let drop = (from - start).clamp(0, frames as i64) as usize;
                offset = drop;
                frames -= drop;
                if frames == 0 {
                    continue;
                }
                self.want_from = None;
            }
            if let Some(rem) = self.remaining.as_mut() {
                frames = frames.min(*rem as usize);
                *rem -= frames as u64;
            }
            if frames == 0 {
                continue;
            }
            let range = offset * ch..(offset + frames) * ch;
            return Ok(Some(match self.spec.format {
                SampleFormat::Int(_) => Chunk::Int(&self.ints[range]),
                _ => Chunk::Float(&self.floats[range]),
            }));
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        let target = (frame + self.delay) as i64;
        // Lossy decoders need preceding audio to converge after a reset. Opus specifies 80 ms,
        // but on steady tones CELT's energy state takes ~250 ms to match a straight decode
        // exactly (measured); MP3's bit reservoir and AAC's overlap need a frame or two.
        // Decoding the pre-roll costs a few milliseconds.
        let preroll = match self.spec.codec {
            "opus" => 12_000,
            "mp3" | "aac" | "vorbis" => 2048,
            _ => 0,
        };
        self.format
            .seek(
                SeekMode::Accurate,
                SeekTo::Timestamp {
                    ts: Timestamp::new((target - preroll).max(0)),
                    track_id: self.track_id,
                },
            )
            .map_err(sym_err)?;
        match &mut self.backend {
            Backend::Symphonia(dec) => dec.reset(),
            Backend::Opus { dec, .. } => dec
                .reset_state()
                .map_err(|e| Error::Decode(e.to_string()))?,
        }
        self.pending = None;
        self.want_from = Some(target);
        if self.remaining.is_some() {
            self.remaining = self.spec.total_frames.map(|t| t.saturating_sub(frame));
        }
        Ok(frame)
    }
}
