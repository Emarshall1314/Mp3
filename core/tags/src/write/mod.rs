//! Writing tags without ever leaving a damaged file behind (ARCHITECTURE §6).
//!
//! 1. Copy the file to a temporary file beside it, and edit the copy.
//! 2. Re-read the copy: every edited field must read back as intended, every other field
//!    exactly as before, and the audio must hash the same, byte for byte.
//! 3. fsync the copy, keep a backup if asked, rename it over the original, fsync the folder.
//!
//! Any failure before the rename leaves the original untouched; a crash leaves at most a stray
//! temporary file (named `.<name>.<pid>.cadenza-tmp`), which [`remove_stale_temps`] clears.
//!
//! Only edited fields are touched: the format's own tag object (ID3v2 frames, Vorbis comments, MP4
//! atoms, APE items) is changed in place, so frames Cadenza doesn't know survive.

mod ape;
mod dsd;
mod id3;
mod mp4;
mod vorbis;

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use lofty::config::{ParseOptions, WriteOptions as LoftyWriteOptions};
use lofty::file::{AudioFile, FileType};
use lofty::probe::Probe;

use crate::{Error, Fields, FileTags, Picture, Result, TagFormat, audio, fields};

/// A change to one file's tags.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Edit {
    /// New values by canonical field name; an empty list removes the field. Fields not listed
    /// are left exactly as they are.
    pub fields: BTreeMap<String, Vec<String>>,
    /// When set, replaces every embedded picture.
    pub pictures: Option<Vec<Picture>>,
}

impl Edit {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.pictures.is_none()
    }
}

#[derive(Debug, Clone, Default)]
pub struct WriteOptions {
    /// Copy the original file here before replacing it.
    pub backup: Option<PathBuf>,
}

/// Fields stored together in some formats ("3/12" in ID3's TRCK, one POPM frame for rating and
/// play count): editing one rewrites both, the other keeping its current value.
const PAIRS: &[(&str, &str)] = &[
    ("tracknumber", "tracktotal"),
    ("discnumber", "disctotal"),
    ("movement", "movementtotal"),
    ("rating", "playcount"),
];

/// Fields that hold one value in every format.
const SINGLE: &[&str] = &[
    "date",
    "originaldate",
    "tracknumber",
    "tracktotal",
    "discnumber",
    "disctotal",
    "movement",
    "movementtotal",
    "compilation",
    "bpm",
    "rating",
    "playcount",
    "comment",
    "lyrics",
];

/// Fields with a number value.
const NUMERIC: &[&str] = &[
    "tracknumber",
    "tracktotal",
    "discnumber",
    "disctotal",
    "movement",
    "movementtotal",
    "rating",
    "playcount",
];

/// Writes `edit` into the file at `path`, safely (see the module docs), and returns the tags as
/// they now read.
pub fn write(path: &Path, edit: &Edit, options: &WriteOptions) -> Result<FileTags> {
    if fs::metadata(path)?.permissions().readonly() {
        return Err(Error::ReadOnly);
    }
    let before = crate::read(path)?;
    if edit.is_empty() {
        return Ok(before);
    }
    let plan = Plan::new(&before, edit)?;
    let audio_hash = audio::content_hash(path, before.audio.container)?;

    let temp = TempFile::beside(path)?;
    if before.audio.container == "dsf" || before.audio.container == "dff" {
        dsd::write(path, temp.path(), &before, &plan)?;
        crash_point("copied");
    } else {
        fs::copy(path, temp.path())?;
        crash_point("copied");
        edit_in_place(temp.path(), &before, &plan)?;
    }
    crash_point("edited");

    let after = crate::read(temp.path())?;
    plan.verify(&before, &after)?;
    if audio::content_hash(temp.path(), after.audio.container)? != audio_hash {
        return Err(Error::Verify("the audio changed".into()));
    }
    File::open(temp.path())?.sync_all()?;
    crash_point("verified");

    if let Some(backup) = &options.backup {
        if let Some(dir) = backup.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::copy(path, backup)?;
        File::open(backup)?.sync_all()?;
    }
    temp.replace(path)?;
    crash_point("replaced");
    Ok(after)
}

/// Removes temporary files left in `dir` by writes that were interrupted (power loss, a crash).
/// Returns how many it removed.
pub fn remove_stale_temps(dir: &Path) -> Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().ends_with(TEMP_SUFFIX) && entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// What a write changes, worked out before touching the file.
#[derive(Debug)]
pub(crate) struct Plan {
    /// Field changes, with pairs completed (see [`PAIRS`]).
    pub changes: BTreeMap<String, Vec<String>>,
    pub pictures: Option<Vec<Picture>>,
    /// The fields the edited file must read back as.
    expected: Fields,
}

impl Plan {
    fn new(before: &FileTags, edit: &Edit) -> Result<Self> {
        let format = written_format(before);
        let mut changes = BTreeMap::new();
        for (field, values) in &edit.fields {
            check_writable(field, format)?;
            let values: Vec<String> = values.iter().filter(|v| !v.is_empty()).cloned().collect();
            if values.len() > 1 && SINGLE.contains(&field.as_str()) {
                return Err(Error::Unwritable(format!("{field} holds a single value")));
            }
            for v in &values {
                check_value(field, v)?;
                let id3 = matches!(format, TagFormat::Id3v23 | TagFormat::Id3v24);
                if id3 && matches!(field.as_str(), "date" | "originaldate") && !is_timestamp(v) {
                    return Err(Error::Unwritable(format!(
                        "{field} must be a date like 2021, 2021-03 or 2021-03-04 in this file"
                    )));
                }
            }
            changes.insert(field.clone(), values);
        }
        for (a, b) in PAIRS {
            for (edited, other) in [(a, b), (b, a)] {
                if changes.contains_key(*edited) && !changes.contains_key(*other) {
                    let current = before.fields.get(*other).cloned().unwrap_or_default();
                    changes.insert((*other).to_owned(), current);
                }
            }
        }
        // A new primary tag replaces one the reader used before (ID3v1 or RIFF INFO): seed it
        // with every current field so nothing disappears.
        if creates_primary(before) {
            for (field, values) in &before.fields {
                if !changes.contains_key(field) && check_writable(field, format).is_ok() {
                    changes.insert(field.clone(), values.clone());
                }
            }
        }

        let mut expected = before.fields.clone();
        for (field, values) in &changes {
            let stored = stored_as(format, field, values, &changes);
            if stored.is_empty() {
                expected.remove(field);
            } else {
                expected.insert(field.clone(), stored);
            }
        }
        if let Some(pictures) = &edit.pictures {
            for p in pictures {
                // FLAC keeps a picture in one metadata block, whose size field is 24 bits (D17).
                if p.data.len() >= (1 << 24) - 64 {
                    return Err(Error::Unwritable(
                        "pictures must be smaller than 16 MB".into(),
                    ));
                }
            }
        }
        Ok(Self {
            changes,
            pictures: edit.pictures.clone(),
            expected,
        })
    }

    fn verify(&self, before: &FileTags, after: &FileTags) -> Result<()> {
        let mut problems = Vec::new();
        let keys: std::collections::BTreeSet<&String> =
            self.expected.keys().chain(after.fields.keys()).collect();
        for key in keys {
            // Values the reader derives from other frames (ID3's UFID) may come and go.
            if key == "musicbrainz_recordingid" && !self.changes.contains_key(key) {
                continue;
            }
            let want = self.expected.get(key);
            let got = after.fields.get(key);
            if want != got {
                problems.push(format!("{key}: wanted {want:?}, read back {got:?}"));
            }
        }
        let same_audio = before.audio.codec == after.audio.codec
            && before.audio.sample_rate == after.audio.sample_rate
            && before.audio.channels == after.audio.channels
            && before.audio.duration_ms == after.audio.duration_ms;
        if !same_audio {
            problems.push("the stream properties changed".into());
        }
        let pictures = self.pictures.as_ref().unwrap_or(&before.pictures);
        let mp4 = after.audio.container == "mp4";
        let same_pictures = pictures.len() == after.pictures.len()
            && pictures
                .iter()
                .zip(&after.pictures)
                .all(|(a, b)| a.data == b.data && (mp4 || a.picture_type == b.picture_type));
        if !same_pictures {
            problems.push("the pictures didn't come back as written".into());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(Error::Verify(problems.join("; ")))
        }
    }
}

/// The tag format a write goes to: the file's primary tag, or the one created for it.
fn written_format(before: &FileTags) -> TagFormat {
    match (before.format, before.audio.container) {
        (TagFormat::Apev2, _) => TagFormat::Apev2,
        (TagFormat::Id3v23, _) => TagFormat::Id3v23,
        (_, "flac" | "ogg") => TagFormat::Vorbis,
        (_, "mp4") => TagFormat::Mp4,
        (_, "ape" | "wv") => TagFormat::Apev2,
        _ => TagFormat::Id3v24,
    }
}

/// Whether the write creates the primary tag while the reader used another source before.
fn creates_primary(before: &FileTags) -> bool {
    match before.format {
        TagFormat::Id3v1 => true,
        // DSDIFF title/artist chunks.
        TagFormat::None => !before.fields.is_empty(),
        // A WAV file with only RIFF INFO reads as "Vorbis" (plain key/value fields).
        TagFormat::Vorbis => before.audio.container == "wav",
        _ => false,
    }
}

fn check_writable(field: &str, format: TagFormat) -> Result<()> {
    let id3 = matches!(format, TagFormat::Id3v23 | TagFormat::Id3v24);
    let ok = match field {
        // Synced lyrics belong in a .lrc file beside the song.
        "syncedlyrics" => false,
        // In ID3 it is the UFID frame, read as the track id too (as mutagen maps it); elsewhere
        // Cadenza has no field for it.
        "musicbrainz_recordingid" => false,
        // ID3 has no place for a performer without an instrument.
        "performer" => !id3,
        f if f.starts_with("performer:") => f.len() > "performer:".len(),
        f if f.starts_with("custom:") => f.len() > "custom:".len(),
        f => fields::is_canonical(f),
    };
    if ok {
        Ok(())
    } else {
        Err(Error::Unwritable(format!(
            "{field} can't be written to this file"
        )))
    }
}

fn check_value(field: &str, value: &str) -> Result<()> {
    if NUMERIC.contains(&field) && value.trim().parse::<u32>().is_err() {
        return Err(Error::Unwritable(format!("{field} must be a whole number")));
    }
    if field == "rating" && value.trim().parse::<u32>().is_ok_and(|v| v > 100) {
        return Err(Error::Unwritable("rating goes from 0 to 100".into()));
    }
    if field == "compilation" && !matches!(value, "0" | "1") {
        return Err(Error::Unwritable("compilation is 1 or 0".into()));
    }
    Ok(())
}

/// ID3v2.4 timestamps: yyyy, yyyy-MM, yyyy-MM-dd, then optionally THH, :mm, :ss.
fn is_timestamp(v: &str) -> bool {
    let b = v.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let shapes: [(usize, &[(usize, u8)]); 6] = [
        (4, &[]),
        (7, &[(4, b'-')]),
        (10, &[(4, b'-'), (7, b'-')]),
        (13, &[(4, b'-'), (7, b'-'), (10, b'T')]),
        (16, &[(4, b'-'), (7, b'-'), (10, b'T'), (13, b':')]),
        (
            19,
            &[(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')],
        ),
    ];
    shapes.iter().any(|(len, seps)| {
        b.len() == *len
            && seps.iter().all(|(i, c)| b[*i] == *c)
            && (0..*len)
                .filter(|i| !seps.iter().any(|(s, _)| s == i))
                .all(|i| digits(i..i + 1))
    })
}

/// How `values` read back once written in `format`.
fn stored_as(
    format: TagFormat,
    field: &str,
    values: &[String],
    changes: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    if values.is_empty() {
        return Vec::new();
    }
    let number = |f: &str| changes.get(f).and_then(|v| v.first()).map(String::as_str);
    let pair_number = match field {
        "tracktotal" => number("tracknumber"),
        "disctotal" => number("discnumber"),
        "movementtotal" => number("movement"),
        _ => Some("1"),
    };
    let id3 = matches!(format, TagFormat::Id3v23 | TagFormat::Id3v24);
    // ID3 and APE store a total only after a number ("3/12").
    if (id3 || format == TagFormat::Apev2) && pair_number.is_none() {
        return Vec::new();
    }
    if NUMERIC.contains(&field) || (format == TagFormat::Mp4 && field == "bpm") {
        let n: u32 = values[0].trim().parse().unwrap_or(0);
        // MP4 keeps 0 as "no value" for its numbers; ID3's POPM likewise for rating and count.
        let zero_is_none =
            format == TagFormat::Mp4 || (id3 && matches!(field, "rating" | "playcount"));
        return if n == 0 && zero_is_none {
            Vec::new()
        } else if format == TagFormat::Mp4 || matches!(field, "rating" | "playcount") {
            vec![n.to_string()]
        } else {
            vec![values[0].trim().to_owned()]
        };
    }
    // ID3v2.3 dates: year, day and month (TDAT needs both), hour and minute; the original date
    // keeps only its year (TORY).
    if format == TagFormat::Id3v23 && matches!(field, "date" | "originaldate") {
        let v = &values[0];
        let keep = match (field, v.len()) {
            ("originaldate", _) | (_, 7) => 4,
            (_, 13) => 10,
            (_, n) if n >= 16 => 16,
            (_, n) => n,
        };
        return vec![v[..keep.min(v.len())].to_owned()];
    }
    // ID3v2.3 has no multiple values: they're joined with "/", as Picard and mutagen write them.
    if format == TagFormat::Id3v23 && values.len() > 1 && !field.starts_with("performer:") {
        return vec![values.join("/")];
    }
    values.to_vec()
}

/// Edits the tags of the file at `path` (a copy) through lofty's concrete types. A file only
/// lofty's relaxed parsing reads (see [`crate::read`]) is edited that way too: the frames it
/// can't parse are dropped.
fn edit_in_place(path: &Path, before: &FileTags, plan: &Plan) -> Result<()> {
    let strict = ParseOptions::new().read_cover_art(true);
    match edit_with(path, before, plan, strict) {
        Err(Error::Malformed(_)) => edit_with(path, before, plan, crate::parse_relaxed()),
        other => other,
    }
}

fn edit_with(path: &Path, before: &FileTags, plan: &Plan, parse: ParseOptions) -> Result<()> {
    use lofty::ape::{ApeFile, ApeTag};
    use lofty::flac::FlacFile;
    use lofty::id3::v2::Id3v2Tag;
    use lofty::iff::aiff::AiffFile;
    use lofty::iff::wav::WavFile;
    use lofty::mp4::{Ilst, Mp4File};
    use lofty::mpeg::MpegFile;
    use lofty::ogg::tag::VorbisComments;
    use lofty::ogg::{OpusFile, VorbisFile};
    use lofty::wavpack::WavPackFile;

    let file_type = Probe::new(BufReader::new(File::open(path)?))
        .guess_file_type()?
        .file_type()
        .ok_or(Error::Unsupported)?;
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let format = written_format(before);
    let v23 = format == TagFormat::Id3v23;
    let mut write = LoftyWriteOptions::new();
    let write = write.use_id3v23(v23);

    macro_rules! id3 {
        ($f:expr) => {{
            if $f.id3v2().is_none() {
                $f.set_id3v2(Id3v2Tag::new());
            }
            let tag = $f.id3v2_mut().ok_or(Error::Unsupported)?;
            id3::apply(tag, plan, v23)?;
        }};
    }
    macro_rules! ape {
        ($f:expr) => {{
            if $f.ape().is_none() {
                $f.set_ape(ApeTag::new());
            }
            let tag = $f.ape_mut().ok_or(Error::Unsupported)?;
            ape::apply(tag, plan)?;
        }};
    }
    macro_rules! save {
        ($f:expr) => {{
            file.seek(SeekFrom::Start(0))?;
            $f.save_to(&mut file, write)
                .map_err(|e| Error::Malformed(e.to_string()))?;
        }};
    }

    match file_type {
        FileType::Mpeg => {
            let mut f = MpegFile::read_from(&mut file, parse)?;
            if format == TagFormat::Apev2 {
                ape!(f);
            } else {
                id3!(f);
            }
            save!(f);
        }
        FileType::Ape => {
            let mut f = ApeFile::read_from(&mut file, parse)?;
            if matches!(format, TagFormat::Id3v23 | TagFormat::Id3v24) {
                id3!(f);
            } else {
                ape!(f);
            }
            save!(f);
        }
        FileType::WavPack => {
            let mut f = WavPackFile::read_from(&mut file, parse)?;
            ape!(f);
            save!(f);
        }
        FileType::Wav => {
            let mut f = WavFile::read_from(&mut file, parse)?;
            id3!(f);
            save!(f);
        }
        FileType::Aiff => {
            let mut f = AiffFile::read_from(&mut file, parse)?;
            id3!(f);
            save!(f);
        }
        FileType::Flac => {
            let mut f = FlacFile::read_from(&mut file, parse)?;
            if f.vorbis_comments().is_none() {
                f.set_vorbis_comments(VorbisComments::new());
            }
            let vc = f.vorbis_comments_mut().ok_or(Error::Unsupported)?;
            vorbis::apply(vc, plan)?;
            if let Some(pictures) = &plan.pictures {
                vorbis::replace_flac_pictures(&mut f, pictures)?;
            }
            save!(f);
        }
        FileType::Opus => {
            let mut f = OpusFile::read_from(&mut file, parse)?;
            vorbis::apply(f.vorbis_comments_mut(), plan)?;
            if let Some(pictures) = &plan.pictures {
                vorbis::replace_pictures(f.vorbis_comments_mut(), pictures)?;
            }
            save!(f);
        }
        FileType::Vorbis => {
            let mut f = VorbisFile::read_from(&mut file, parse)?;
            vorbis::apply(f.vorbis_comments_mut(), plan)?;
            if let Some(pictures) = &plan.pictures {
                vorbis::replace_pictures(f.vorbis_comments_mut(), pictures)?;
            }
            save!(f);
        }
        FileType::Mp4 => {
            let mut f = Mp4File::read_from(&mut file, parse)?;
            if f.ilst().is_none() {
                f.set_ilst(Ilst::new());
            }
            let ilst = f.ilst_mut().ok_or(Error::Unsupported)?;
            mp4::apply(ilst, plan)?;
            save!(f);
        }
        _ => return Err(Error::Unsupported),
    }
    Ok(())
}

/// lofty's picture from ours. A missing MIME type is taken from the data.
pub(crate) fn lofty_picture(p: &Picture) -> lofty::picture::Picture {
    use lofty::picture::{MimeType, Picture as LoftyPicture, PictureType};
    let mime = if p.mime.is_empty() {
        if p.data.starts_with(&[0x89, b'P', b'N', b'G']) {
            MimeType::Png
        } else {
            MimeType::Jpeg
        }
    } else {
        MimeType::from_str(&p.mime)
    };
    let mut builder = LoftyPicture::unchecked(p.data.clone())
        .pic_type(PictureType::from_u8(p.picture_type))
        .mime_type(mime);
    if !p.description.is_empty() {
        builder = builder.description(p.description.clone());
    }
    builder.build()
}

const TEMP_SUFFIX: &str = ".cadenza-tmp";

/// A temporary file beside the original, removed unless it replaces the original.
struct TempFile {
    path: PathBuf,
    kept: bool,
}

impl TempFile {
    fn beside(original: &Path) -> Result<Self> {
        let name = original
            .file_name()
            .ok_or(Error::Unsupported)?
            .to_string_lossy()
            .into_owned();
        let path = original.with_file_name(format!(".{name}.{}{TEMP_SUFFIX}", std::process::id()));
        Ok(Self { path, kept: false })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Atomically puts this file in place of `original`, and makes the rename durable.
    fn replace(mut self, original: &Path) -> Result<()> {
        fs::rename(&self.path, original)?;
        self.kept = true;
        if let Some(dir) = original.parent() {
            let dir = if dir.as_os_str().is_empty() {
                Path::new(".")
            } else {
                dir
            };
            File::open(dir)?.sync_all()?;
        }
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.kept {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Test hook: aborts the process at a named stage when `CADENZA_TAGS_CRASH_AT` asks for it,
/// standing in for a crash or power loss mid-write. Debug builds only.
fn crash_point(stage: &str) {
    #[cfg(debug_assertions)]
    if std::env::var("CADENZA_TAGS_CRASH_AT").as_deref() == Ok(stage) {
        std::process::abort();
    }
    #[cfg(not(debug_assertions))]
    let _ = stage;
}
