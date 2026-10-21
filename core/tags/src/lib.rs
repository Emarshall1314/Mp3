//! Tag and audio-property reading for every format Cadenza plays.
//!
//! Tags are read through lofty's *concrete* tag types (`Id3v2Tag`, `VorbisComments`, `Ilst`,
//! `ApeTag`), never its lossy generic `Tag`, and mapped onto Cadenza's canonical field names
//! (see [`fields`]). Anything unrecognised becomes a `custom:<NAME>` field rather than being
//! dropped. DSF/DFF, which lofty does not support, are handled in [`dsd`].

mod ape;
mod dsd;
pub mod fields;
mod id3;
mod legacy;
mod mp4;
mod vorbis;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType};
use lofty::ogg::OggPictureStorage;
use lofty::probe::Probe;
use serde::Serialize;

pub use legacy::LegacyText;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a supported audio file")]
    Unsupported,
    #[error("malformed file: {0}")]
    Malformed(String),
}

impl From<lofty::error::FileParseError> for Error {
    fn from(e: lofty::error::FileParseError) -> Self {
        Error::Malformed(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The tag format a file's primary tag uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TagFormat {
    Id3v22,
    Id3v23,
    Id3v24,
    Vorbis,
    Mp4,
    Apev2,
    Id3v1,
    None,
}

/// Field values by canonical name; every field may hold several values.
pub type Fields = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Picture {
    /// ID3/FLAC picture type: 3 = front cover, 4 = back cover, 8 = artist ...
    pub picture_type: u8,
    pub mime: String,
    pub description: String,
    #[serde(skip)]
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Codec {
    Pcm,
    Flac,
    Alac,
    Ape,
    WavPack,
    Dsd,
    Mp3,
    Aac,
    Opus,
    Vorbis,
    Other,
}

impl Codec {
    pub fn is_lossless(self) -> bool {
        matches!(
            self,
            Codec::Pcm | Codec::Flac | Codec::Alac | Codec::Ape | Codec::WavPack | Codec::Dsd
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Codec::Pcm => "pcm",
            Codec::Flac => "flac",
            Codec::Alac => "alac",
            Codec::Ape => "ape",
            Codec::WavPack => "wavpack",
            Codec::Dsd => "dsd",
            Codec::Mp3 => "mp3",
            Codec::Aac => "aac",
            Codec::Opus => "opus",
            Codec::Vorbis => "vorbis",
            Codec::Other => "other",
        }
    }
}

/// Stream facts as recorded in the container headers (the decoder is authoritative for playback).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioInfo {
    pub container: &'static str,
    pub codec: Codec,
    pub sample_rate: u32,
    /// Bits per sample for PCM-like codecs; `None` for lossy codecs.
    pub bit_depth: Option<u8>,
    pub float: bool,
    pub channels: u8,
    pub duration_ms: u64,
    pub bitrate_kbps: u32,
    /// DSD sample rate (e.g. 2 822 400 for DSD64) when the stream is DSD.
    pub dsd_rate: Option<u32>,
}

/// Everything read from one file.
#[derive(Debug, Clone, Serialize)]
pub struct FileTags {
    pub format: TagFormat,
    pub fields: Fields,
    pub pictures: Vec<Picture>,
    /// Text that looks like a legacy 8-bit encoding stored as Latin-1, with a suggested repair.
    pub legacy: Vec<LegacyText>,
    pub audio: AudioInfo,
}

impl FileTags {
    pub fn get(&self, field: &str) -> Option<&str> {
        self.fields
            .get(field)
            .and_then(|v| v.first())
            .map(String::as_str)
    }

    pub fn front_cover(&self) -> Option<&Picture> {
        self.pictures
            .iter()
            .find(|p| p.picture_type == 3)
            .or_else(|| self.pictures.first())
    }
}

/// Builder used by the per-format readers.
#[derive(Debug, Default)]
pub(crate) struct FieldsBuilder {
    pub fields: Fields,
}

impl FieldsBuilder {
    pub fn push(&mut self, field: &str, value: &str) {
        if value.is_empty() {
            return;
        }
        self.fields
            .entry(field.to_owned())
            .or_default()
            .push(value.to_owned());
    }

    pub fn push_custom(&mut self, name: &str, value: &str) {
        self.push(&format!("custom:{name}"), value);
    }

    /// "3/12" → number 3, total 12. Totals already set by a dedicated key are not duplicated.
    pub fn push_number_pair(&mut self, number_field: &str, total_field: &str, value: &str) {
        let (n, total) = match value.split_once('/') {
            Some((n, t)) => (n.trim(), Some(t.trim())),
            None => (value.trim(), None),
        };
        self.push(number_field, n);
        if let Some(t) = total.filter(|t| !t.is_empty())
            && !self.fields.contains_key(total_field)
        {
            self.push(total_field, t);
        }
    }

    pub fn finish(mut self) -> Fields {
        for values in self.fields.values_mut() {
            // A total written both as "3/12" and as a TRACKTOTAL key reads identically; keep one.
            values.dedup();
        }
        self.fields
    }
}

/// "Name (role)" → ("Name", "role"), the Vorbis/APE performer convention.
pub(crate) fn split_performer(value: &str) -> (&str, Option<&str>) {
    if let Some(stripped) = value.strip_suffix(')')
        && let Some(open) = stripped.rfind(" (")
    {
        return (&stripped[..open], Some(&stripped[open + 2..]));
    }
    (value, None)
}

/// Converts a 0..1 (FMPS) or 0..100 rating string to Cadenza's 0..100 scale.
pub(crate) fn normalise_rating(value: &str) -> Option<String> {
    let v: f64 = value.trim().parse().ok()?;
    let scaled = if v <= 1.0 { v * 100.0 } else { v };
    (0.0..=100.0)
        .contains(&scaled)
        .then(|| format!("{}", scaled.round() as u32))
}

/// Reads tags, pictures and stream facts. The format is sniffed from the content, so a FLAC file
/// named `.mp3` reads as FLAC.
pub fn read(path: &Path) -> Result<FileTags> {
    let mut file = File::open(path)?;
    let mut magic = [0u8; 4];
    let n = file.read(&mut magic)?;
    file.seek(SeekFrom::Start(0))?;
    if n == 4 && (&magic == b"DSD " || &magic == b"FRM8") {
        return dsd::read(&mut BufReader::new(file), &magic);
    }

    let probe = Probe::new(BufReader::new(file)).guess_file_type()?;
    let file_type = probe.file_type().ok_or(Error::Unsupported)?;
    let mut reader = probe.into_inner();
    let opts = ParseOptions::new().read_cover_art(true);
    read_typed(&mut reader, file_type, opts)
}

fn read_typed<R: Read + Seek>(r: &mut R, ft: FileType, opts: ParseOptions) -> Result<FileTags> {
    use lofty::ape::ApeFile;
    use lofty::flac::FlacFile;
    use lofty::iff::aiff::AiffFile;
    use lofty::iff::wav::WavFile;
    use lofty::mp4::{Mp4Codec, Mp4File};
    use lofty::mpeg::MpegFile;
    use lofty::ogg::{OpusFile, VorbisFile};
    use lofty::wavpack::WavPackFile;

    let mut legacy = Vec::new();
    let (format, fields, pictures, audio) = match ft {
        FileType::Mpeg => {
            let f = MpegFile::read_from(r, opts)?;
            let p = f.properties();
            let audio = info(
                "mpeg",
                Codec::Mp3,
                p.sample_rate(),
                None,
                p.channels(),
                p.duration(),
                p.audio_bitrate(),
            );
            match (f.id3v2(), f.ape(), f.id3v1()) {
                (Some(t), _, _) => with_id3(t, &mut legacy, audio),
                (None, Some(t), _) => with_ape(t, audio),
                (None, None, Some(t)) => with_id3v1(t, &mut legacy, audio),
                (None, None, None) => empty(audio),
            }
        }
        FileType::Flac => {
            let f = FlacFile::read_from(r, opts)?;
            let p = f.properties();
            let audio = info(
                "flac",
                Codec::Flac,
                p.sample_rate(),
                Some(p.bit_depth()),
                p.channels(),
                p.duration(),
                p.audio_bitrate(),
            );
            let mut pictures: Vec<Picture> = f
                .pictures()
                .iter()
                .map(|(p, _)| vorbis::picture(p))
                .collect();
            match f.vorbis_comments() {
                Some(vc) => {
                    let (fields, more) = vorbis::read(vc);
                    pictures.extend(more);
                    (TagFormat::Vorbis, fields, pictures, audio)
                }
                None => (TagFormat::None, Fields::new(), pictures, audio),
            }
        }
        FileType::Opus | FileType::Vorbis => {
            let (vc, audio) = if ft == FileType::Opus {
                let f = OpusFile::read_from(r, opts)?;
                let p = f.properties();
                let a = info(
                    "ogg",
                    Codec::Opus,
                    48_000,
                    None,
                    p.channels(),
                    p.duration(),
                    p.audio_bitrate(),
                );
                (f.vorbis_comments().clone(), a)
            } else {
                let f = VorbisFile::read_from(r, opts)?;
                let p = f.properties();
                let a = info(
                    "ogg",
                    Codec::Vorbis,
                    p.sample_rate(),
                    None,
                    p.channels(),
                    p.duration(),
                    p.audio_bitrate(),
                );
                (f.vorbis_comments().clone(), a)
            };
            let (fields, pictures) = vorbis::read(&vc);
            (TagFormat::Vorbis, fields, pictures, audio)
        }
        FileType::Mp4 => {
            let f = Mp4File::read_from(r, opts)?;
            let p = f.properties();
            let (codec, depth) = match p.codec() {
                Some(Mp4Codec::ALAC) => (Codec::Alac, p.bit_depth()),
                Some(Mp4Codec::FLAC) => (Codec::Flac, p.bit_depth()),
                Some(Mp4Codec::AAC) => (Codec::Aac, None),
                _ => (Codec::Other, None),
            };
            let audio = info(
                "mp4",
                codec,
                p.sample_rate().unwrap_or(0),
                depth,
                p.channels().unwrap_or(0),
                p.duration(),
                p.audio_bitrate().unwrap_or(0),
            );
            match f.ilst() {
                Some(ilst) => {
                    let (fields, pictures) = mp4::read(ilst);
                    (TagFormat::Mp4, fields, pictures, audio)
                }
                None => empty(audio),
            }
        }
        FileType::Ape => {
            let f = ApeFile::read_from(r, opts)?;
            let p = f.properties();
            let audio = info(
                "ape",
                Codec::Ape,
                p.sample_rate(),
                Some(p.bit_depth()),
                p.channels(),
                p.duration(),
                p.bitrate(),
            );
            match (f.ape(), f.id3v2(), f.id3v1()) {
                (Some(t), _, _) => with_ape(t, audio),
                (None, Some(t), _) => with_id3(t, &mut legacy, audio),
                (None, None, Some(t)) => with_id3v1(t, &mut legacy, audio),
                _ => empty(audio),
            }
        }
        FileType::WavPack => {
            let f = WavPackFile::read_from(r, opts)?;
            let p = f.properties();
            let flags = wavpack_flags(r)?;
            let mut audio = info(
                "wv",
                Codec::WavPack,
                p.sample_rate(),
                Some(p.bit_depth()),
                p.channels(),
                p.duration(),
                p.audio_bitrate(),
            );
            audio.float = flags.float;
            if let Some(ch) = flags.channels {
                audio.channels = ch;
            }
            if flags.dsd {
                // WavPack reports the DSD byte rate; the bit rate is 8x.
                audio.dsd_rate = Some(p.sample_rate() * 8);
                audio.bit_depth = Some(1);
                audio.codec = Codec::Dsd;
            }
            match (f.ape(), f.id3v1()) {
                (Some(t), _) => with_ape(t, audio),
                (None, Some(t)) => with_id3v1(t, &mut legacy, audio),
                _ => empty(audio),
            }
        }
        FileType::Wav => {
            let f = WavFile::read_from(r, opts)?;
            let p = f.properties();
            let mut audio = info(
                "wav",
                Codec::Pcm,
                p.sample_rate(),
                Some(p.bit_depth()),
                p.channels(),
                p.duration(),
                p.bitrate(),
            );
            audio.float = matches!(p.format(), lofty::iff::wav::WavFormat::IEEE_FLOAT);
            match f.id3v2() {
                Some(t) => with_id3(t, &mut legacy, audio),
                None => {
                    let fields = f.riff_info().map(id3::riff_info).unwrap_or_default();
                    let format = if fields.is_empty() {
                        TagFormat::None
                    } else {
                        TagFormat::Vorbis
                    };
                    (format, fields, Vec::new(), audio)
                }
            }
        }
        FileType::Aiff => {
            let f = AiffFile::read_from(r, opts)?;
            let p = f.properties();
            let audio = info(
                "aiff",
                Codec::Pcm,
                p.sample_rate(),
                Some(p.sample_size() as u8),
                p.channels(),
                p.duration(),
                p.audio_bitrate(),
            );
            match f.id3v2() {
                Some(t) => with_id3(t, &mut legacy, audio),
                None => empty(audio),
            }
        }
        _ => return Err(Error::Unsupported),
    };
    Ok(FileTags {
        format,
        fields,
        pictures,
        legacy,
        audio,
    })
}

type Parts = (TagFormat, Fields, Vec<Picture>, AudioInfo);

fn empty(audio: AudioInfo) -> Parts {
    (TagFormat::None, Fields::new(), Vec::new(), audio)
}

fn with_id3(t: &lofty::id3::v2::Id3v2Tag, legacy: &mut Vec<LegacyText>, audio: AudioInfo) -> Parts {
    let (format, fields, pictures) = id3::read(t, legacy);
    (format, fields, pictures, audio)
}

fn with_id3v1(
    t: &lofty::id3::v1::Id3v1Tag,
    legacy: &mut Vec<LegacyText>,
    audio: AudioInfo,
) -> Parts {
    (TagFormat::Id3v1, id3::read_v1(t, legacy), Vec::new(), audio)
}

fn with_ape(t: &lofty::ape::ApeTag, audio: AudioInfo) -> Parts {
    let (fields, pictures) = ape::read(t);
    (TagFormat::Apev2, fields, pictures, audio)
}

fn info(
    container: &'static str,
    codec: Codec,
    sample_rate: u32,
    bit_depth: Option<u8>,
    channels: impl Into<u16>,
    duration: std::time::Duration,
    bitrate: u32,
) -> AudioInfo {
    AudioInfo {
        container,
        codec,
        sample_rate,
        bit_depth: if codec.is_lossless() { bit_depth } else { None },
        float: false,
        channels: channels.into().min(255) as u8,
        duration_ms: duration.as_millis() as u64,
        bitrate_kbps: bitrate,
        dsd_rate: None,
    }
}

struct WavPackFlags {
    float: bool,
    dsd: bool,
    /// From the ID_CHANNEL_INFO sub-block; lofty misses it for more than two channels.
    channels: Option<u8>,
}

/// Reads the flags word of the first WavPack block header ("wvpk", flags at offset 24).
fn wavpack_flags<R: Read + Seek>(r: &mut R) -> Result<WavPackFlags> {
    r.seek(SeekFrom::Start(0))?;
    let mut header = [0u8; 32];
    r.read_exact(&mut header)?;
    if &header[..4] != b"wvpk" {
        return Err(Error::Malformed("WavPack block header not at start".into()));
    }
    let flags = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
    let block_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    // Metadata sub-blocks follow the 32-byte header: [id][size in words (1 or 3 bytes)][data].
    let mut body = vec![0u8; block_size.saturating_sub(24).min(1 << 20)];
    let n = r.read(&mut body)?;
    body.truncate(n);
    let mut channels = None;
    let mut p = 0;
    while p + 2 <= body.len() {
        let id = body[p];
        let (words, head) = if id & 0x80 != 0 && p + 4 <= body.len() {
            (
                body[p + 1] as usize | (body[p + 2] as usize) << 8 | (body[p + 3] as usize) << 16,
                4,
            )
        } else {
            (body[p + 1] as usize, 2)
        };
        let data = p + head;
        if id & 0x3f == 0x0d && data < body.len() {
            channels = Some(body[data]);
        }
        p = data + words * 2;
    }
    Ok(WavPackFlags {
        float: flags & 0x80 != 0,
        dsd: flags & 0x8000_0000 != 0,
        channels,
    })
}
