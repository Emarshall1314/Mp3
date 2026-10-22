//! Tag editing: reading a track's tags for the editor, and writing edits to files in the
//! background with progress, cancellation and undo.
//!
//! Each file is written with `cadenza_tags::write` (never damaged, verified before it replaces
//! the original). What changed is recorded per batch in the library, and undoing a batch writes
//! the old values back, skipping fields changed since. Afterwards an incremental scan brings the
//! library up to date.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use cadenza_library::Library;
use cadenza_tags::{Edit, WriteOptions};
use serde::{Deserialize, Serialize};

use crate::{ApiEvent, Emit, Error, Result, Scanner};

/// New values for one track's fields; an empty list clears a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackEdit {
    pub track_id: i64,
    pub fields: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub pictures: Option<PictureEdit>,
}

/// A change to a track's embedded pictures. Images are referenced by their artwork-cache hash
/// (see [`TrackTags::pictures`] and `Cadenza::import_artwork`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PictureEdit {
    /// Exactly these pictures, in this order.
    Replace {
        pictures: Vec<PictureRef>,
    },
    /// This image as the front cover; the track's other pictures stay.
    SetFront {
        hash: String,
    },
    RemoveAll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureRef {
    pub picture_type: u8,
    pub hash: String,
    #[serde(default)]
    pub description: String,
}

/// The history "field" for pictures: each as "type:hash:description".
const PICTURES: &str = "pictures";

/// A track's tags as the file holds them now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackTags {
    pub track_id: i64,
    pub path: String,
    /// "id3v2.4", "vorbis", "mp4", ...
    pub format: String,
    pub fields: cadenza_tags::Fields,
    pub pictures: Vec<PictureInfo>,
    /// Text that looks like a legacy encoding (Shift-JIS, CP1251...) stored as Latin-1.
    pub legacy: Vec<LegacyFix>,
    /// Why the tags can't be edited (a song from a cue sheet, a read-only file), if so.
    pub read_only: Option<String>,
}

/// A suggested repair: field values equal to `stored` read correctly as `decoded`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyFix {
    pub stored: String,
    pub decoded: String,
    /// WHATWG encoding name, e.g. "Shift_JIS".
    pub encoding: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureInfo {
    pub picture_type: u8,
    pub mime: String,
    pub description: String,
    pub bytes: usize,
    /// In the artwork cache, for display and for keeping the picture in an edit; `None` if the
    /// image is damaged.
    pub hash: Option<String>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditStatus {
    pub running: bool,
    pub batch: Option<i64>,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditFailure {
    pub track_id: i64,
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditReport {
    pub batch: i64,
    pub written: u64,
    pub failed: Vec<EditFailure>,
    pub cancelled: bool,
}

/// One file to write: new values, and for undo the values the file must still hold.
struct Job {
    track_id: i64,
    fields: BTreeMap<String, Vec<String>>,
    expect: BTreeMap<String, Vec<String>>,
    pictures: Option<PictureEdit>,
    /// For undo: the pictures the file must still hold (as recorded).
    expect_pictures: Option<Vec<String>>,
    /// Instead of tags: move the file from here to there (the organiser, and its undo).
    rename: Option<(PathBuf, PathBuf)>,
}

/// One file's move in an organiser preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePlan {
    pub track_id: i64,
    pub from: String,
    pub to: String,
    /// "move", "same" (already there), "conflict" or "error"; only "move" happens.
    pub status: String,
    /// Why, for a conflict or an error.
    pub note: Option<String>,
}

/// The history "field" for a file move: [old path] → [new path].
const FILE: &str = "file";

pub(crate) struct Editor {
    library: Arc<Library>,
    scanner: Arc<Scanner>,
    emit: Emit,
    data_dir: PathBuf,
    running: AtomicBool,
    cancel: AtomicBool,
    done: AtomicU64,
    total: AtomicU64,
    batch: Mutex<Option<i64>>,
}

impl Editor {
    pub(crate) fn new(
        library: Arc<Library>,
        scanner: Arc<Scanner>,
        emit: Emit,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            library,
            scanner,
            emit,
            data_dir,
            running: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            batch: Mutex::new(None),
        }
    }

    pub(crate) fn status(&self) -> EditStatus {
        EditStatus {
            running: self.running.load(Ordering::Acquire),
            batch: self.batch.lock().ok().and_then(|b| *b),
            done: self.done.load(Ordering::Acquire),
            total: self.total.load(Ordering::Acquire),
        }
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub(crate) fn tags(&self, ids: &[i64]) -> Result<Vec<TrackTags>> {
        let mut out = Vec::new();
        for t in self.library.playable(ids)? {
            let tags = cadenza_tags::read(&t.path).map_err(|e| Error::Other(e.to_string()))?;
            let read_only = if t.start_frame.is_some() {
                Some("This song is part of a single-file album described by a cue sheet; its details come from the .cue file.".to_owned())
            } else if std::fs::metadata(&t.path).is_ok_and(|m| m.permissions().readonly()) {
                Some("The file is read-only.".to_owned())
            } else {
                None
            };
            out.push(TrackTags {
                track_id: t.id,
                path: t.path.display().to_string(),
                format: format_name(tags.format).to_owned(),
                legacy: tags
                    .legacy
                    .iter()
                    .map(|l| LegacyFix {
                        stored: l.raw.iter().map(|&b| b as char).collect(),
                        decoded: l.decoded.clone(),
                        encoding: l.encoding.to_owned(),
                    })
                    .collect(),
                fields: tags.fields,
                pictures: tags
                    .pictures
                    .iter()
                    .map(|p| {
                        let stored = self.library.store_artwork(&p.data).ok().flatten();
                        PictureInfo {
                            picture_type: p.picture_type,
                            mime: p.mime.clone(),
                            description: p.description.clone(),
                            bytes: p.data.len(),
                            width: stored.as_ref().map_or(0, |s| s.width),
                            height: stored.as_ref().map_or(0, |s| s.height),
                            hash: stored.map(|s| s.hash),
                        }
                    })
                    .collect(),
                read_only,
            });
        }
        Ok(out)
    }

    /// Starts writing `edits` in the background; returns the batch id.
    pub(crate) fn start(
        self: &Arc<Self>,
        edits: Vec<TrackEdit>,
        description: &str,
        backups: bool,
    ) -> Result<i64> {
        let jobs = edits
            .into_iter()
            .filter(|e| !e.fields.is_empty() || e.pictures.is_some())
            .map(|e| Job {
                track_id: e.track_id,
                fields: e.fields,
                expect: BTreeMap::new(),
                pictures: e.pictures,
                expect_pictures: None,
                rename: None,
            })
            .collect();
        self.run(jobs, description, None, backups)
    }

    /// Writes back what `batch` changed, where the files still hold what it wrote.
    pub(crate) fn undo(self: &Arc<Self>, batch: i64, backups: bool) -> Result<i64> {
        let original = self
            .library
            .edit_batches(1000)?
            .into_iter()
            .find(|b| b.id == batch)
            .ok_or_else(|| Error::Other("that edit is no longer in the history".into()))?;
        if original.undone {
            return Err(Error::Other("that edit was already undone".into()));
        }
        let mut jobs: BTreeMap<i64, Job> = BTreeMap::new();
        let mut gone = 0;
        for change in self.library.edit_changes(batch)? {
            let Some(track_id) = change.track_id else {
                gone += 1;
                continue;
            };
            let job = jobs.entry(track_id).or_insert_with(|| Job {
                track_id,
                fields: BTreeMap::new(),
                expect: BTreeMap::new(),
                pictures: None,
                expect_pictures: None,
                rename: None,
            });
            if change.field == FILE {
                if let (Some(old), Some(new)) = (change.old.first(), change.new.first()) {
                    job.rename = Some((PathBuf::from(new), PathBuf::from(old)));
                }
            } else if change.field == PICTURES {
                job.pictures = Some(PictureEdit::Replace {
                    pictures: change.old.iter().filter_map(|s| parse_picture(s)).collect(),
                });
                job.expect_pictures = Some(change.new);
            } else {
                job.fields.insert(change.field.clone(), change.old);
                job.expect.insert(change.field, change.new);
            }
        }
        if jobs.is_empty() {
            return Err(Error::Other(if gone > 0 {
                "the edited songs are no longer in the library".into()
            } else {
                "that edit changed nothing".into()
            }));
        }
        let description = format!("Undo “{}”", original.description);
        self.run(
            jobs.into_values().collect(),
            &description,
            Some(batch),
            backups,
        )
    }

    fn run(
        self: &Arc<Self>,
        jobs: Vec<Job>,
        description: &str,
        undo_of: Option<i64>,
        backups: bool,
    ) -> Result<i64> {
        if jobs.is_empty() {
            return Err(Error::Other("nothing to change".into()));
        }
        if self.running.swap(true, Ordering::AcqRel) {
            return Err(Error::Other(
                "other tag changes are still being written".into(),
            ));
        }
        let batch = match self.library.begin_edit_batch(description, undo_of) {
            Ok(b) => b,
            Err(e) => {
                self.running.store(false, Ordering::Release);
                return Err(e.into());
            }
        };
        self.cancel.store(false, Ordering::Release);
        self.done.store(0, Ordering::Release);
        self.total.store(jobs.len() as u64, Ordering::Release);
        if let Ok(mut b) = self.batch.lock() {
            *b = Some(batch);
        }
        let me = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("cadenza-tag-writer".into())
            .spawn(move || me.work(batch, jobs, backups));
        if let Err(e) = spawned {
            self.running.store(false, Ordering::Release);
            return Err(Error::Other(e.to_string()));
        }
        Ok(batch)
    }

    fn work(&self, batch: i64, jobs: Vec<Job>, backups: bool) {
        let mut written = 0;
        let mut failed = Vec::new();
        let mut cancelled = false;
        self.progress();
        for job in jobs {
            if self.cancel.load(Ordering::Acquire) {
                cancelled = true;
                break;
            }
            match self.write_one(batch, &job, backups) {
                Ok(()) => written += 1,
                Err((path, error)) => failed.push(EditFailure {
                    track_id: job.track_id,
                    path,
                    error,
                }),
            }
            self.done.fetch_add(1, Ordering::AcqRel);
            self.progress();
        }
        let _ = self
            .library
            .finish_edit_batch(batch, written as i64, failed.len() as i64);
        self.running.store(false, Ordering::Release);
        (self.emit)(ApiEvent::EditFinished {
            report: EditReport {
                batch,
                written,
                failed,
                cancelled,
            },
        });
        if written > 0 {
            self.scanner.request();
        }
    }

    fn progress(&self) {
        (self.emit)(ApiEvent::EditProgress {
            status: self.status(),
        });
    }

    /// Writes one file; on failure, its path and why.
    fn write_one(
        &self,
        batch: i64,
        job: &Job,
        backups: bool,
    ) -> std::result::Result<(), (String, String)> {
        let track = self
            .library
            .playable(&[job.track_id])
            .ok()
            .and_then(|t| t.into_iter().next())
            .ok_or_else(|| {
                (
                    String::new(),
                    "the song is no longer in the library".to_owned(),
                )
            })?;
        let path = track.path.display().to_string();
        let fail = |e: String| (path.clone(), e);
        if let Some((from, to)) = &job.rename {
            return self.move_file(batch, job.track_id, from, to);
        }
        if track.start_frame.is_some() {
            return Err(fail(
                "songs from a cue sheet are edited in the .cue file".into(),
            ));
        }
        let before = cadenza_tags::read(&track.path).map_err(|e| fail(e.to_string()))?;
        for (field, expected) in &job.expect {
            let now = before.fields.get(field).cloned().unwrap_or_default();
            if &now != expected {
                return Err(fail(format!("{field} has changed since; left as it is")));
            }
        }
        let backup = backups.then(|| {
            let name = track
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.data_dir
                .join("backups")
                .join(batch.to_string())
                .join(format!("{}-{name}", job.track_id))
        });
        let old_pictures = self.signature(&before.pictures);
        if let Some(expected) = &job.expect_pictures
            && &old_pictures != expected
        {
            return Err(fail("the artwork has changed since; left as it is".into()));
        }
        let pictures = match &job.pictures {
            Some(edit) => Some(self.resolve(edit, &before.pictures).map_err(fail)?),
            None => None,
        };
        let edit = Edit {
            fields: job.fields.clone(),
            pictures,
        };
        let after = cadenza_tags::write(&track.path, &edit, &WriteOptions { backup })
            .map_err(|e| fail(e.to_string()))?;
        let mut changes: Vec<(String, Vec<String>, Vec<String>)> = job
            .fields
            .keys()
            .map(|field| {
                let old = before.fields.get(field).cloned().unwrap_or_default();
                let new = after.fields.get(field).cloned().unwrap_or_default();
                (field.clone(), old, new)
            })
            .filter(|(_, old, new)| old != new)
            .collect();
        if job.pictures.is_some() {
            let new_pictures = self.signature(&after.pictures);
            if new_pictures != old_pictures {
                changes.push((PICTURES.to_owned(), old_pictures, new_pictures));
            }
        }
        if !changes.is_empty() {
            self.library
                .record_edit(batch, job.track_id, changes)
                .map_err(|e| fail(format!("written, but not recorded for undo: {e}")))?;
        }
        Ok(())
    }
}

impl Editor {
    /// Pictures as recorded in history: "type:hash:description", the image kept in the
    /// artwork cache so an undo can put it back. Damaged images record as "type:?:...".
    fn signature(&self, pictures: &[cadenza_tags::Picture]) -> Vec<String> {
        pictures
            .iter()
            .map(|p| {
                let hash = self
                    .library
                    .store_artwork(&p.data)
                    .ok()
                    .flatten()
                    .map_or_else(|| "?".to_owned(), |s| s.hash);
                format!("{}:{hash}:{}", p.picture_type, p.description)
            })
            .collect()
    }

    /// The pictures an edit asks for, read from the artwork cache.
    fn resolve(
        &self,
        edit: &PictureEdit,
        current: &[cadenza_tags::Picture],
    ) -> std::result::Result<Vec<cadenza_tags::Picture>, String> {
        let load = |r: &PictureRef| -> std::result::Result<cadenza_tags::Picture, String> {
            let path = self
                .library
                .artwork_original(&r.hash)
                .map_err(|_| "an image for this edit is no longer in Cadenza's cache".to_owned())?;
            let data = std::fs::read(&path).map_err(|e| e.to_string())?;
            let mime = match path.extension().and_then(|e| e.to_str()) {
                Some("png") => "image/png",
                Some("webp") => "image/webp",
                Some("gif") => "image/gif",
                Some("bmp") => "image/bmp",
                _ => "image/jpeg",
            };
            Ok(cadenza_tags::Picture {
                picture_type: r.picture_type,
                mime: mime.to_owned(),
                description: r.description.clone(),
                data,
            })
        };
        Ok(match edit {
            PictureEdit::Replace { pictures } => pictures
                .iter()
                .map(load)
                .collect::<std::result::Result<_, _>>()?,
            PictureEdit::SetFront { hash } => {
                let front = load(&PictureRef {
                    picture_type: 3,
                    hash: hash.clone(),
                    description: String::new(),
                })?;
                std::iter::once(front)
                    .chain(current.iter().filter(|p| p.picture_type != 3).cloned())
                    .collect()
            }
            PictureEdit::RemoveAll => Vec::new(),
        })
    }

    /// Where the organiser would put each track's file under `pattern` (relative to its music
    /// folder), without touching anything.
    pub(crate) fn preview_organise(&self, ids: &[i64], pattern: &str) -> Result<Vec<RenamePlan>> {
        let roots: Vec<PathBuf> = self
            .library
            .roots()?
            .into_iter()
            .map(|r| PathBuf::from(r.path))
            .collect();
        let mut plans: Vec<RenamePlan> = Vec::new();
        let mut taken = std::collections::HashSet::new();
        for t in self.library.playable(ids)? {
            let from = t.path.display().to_string();
            let mut plan = RenamePlan {
                track_id: t.id,
                from: from.clone(),
                to: String::new(),
                status: "error".into(),
                note: None,
            };
            let target = (|| -> std::result::Result<PathBuf, String> {
                if t.start_frame.is_some() {
                    return Err("part of a single-file album with a cue sheet".into());
                }
                let root = roots
                    .iter()
                    .filter(|r| t.path.starts_with(r))
                    .max_by_key(|r| r.as_os_str().len())
                    .ok_or("not inside a music folder")?;
                let tags = cadenza_tags::read(&t.path).map_err(|e| e.to_string())?;
                let ext = t
                    .path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default();
                Ok(root.join(crate::organise::relative_path(pattern, &tags.fields, ext)?))
            })();
            match target {
                Err(note) => plan.note = Some(note),
                Ok(to) => {
                    plan.to = to.display().to_string();
                    if to == t.path {
                        plan.status = "same".into();
                    } else if !taken.insert(to.clone()) {
                        plan.status = "conflict".into();
                        plan.note =
                            Some("another song in this list would get the same name".into());
                    } else if to.exists() {
                        plan.status = "conflict".into();
                        plan.note = Some("a file with that name is already there".into());
                    } else {
                        plan.status = "move".into();
                    }
                }
            }
            plans.push(plan);
        }
        Ok(plans)
    }

    /// Moves the files of a preview's "move" entries, in the background (a batch, undoable).
    pub(crate) fn organise(self: &Arc<Self>, ids: &[i64], pattern: &str) -> Result<i64> {
        let jobs: Vec<Job> = self
            .preview_organise(ids, pattern)?
            .into_iter()
            .filter(|p| p.status == "move")
            .map(|p| Job {
                track_id: p.track_id,
                fields: BTreeMap::new(),
                expect: BTreeMap::new(),
                pictures: None,
                expect_pictures: None,
                rename: Some((PathBuf::from(p.from), PathBuf::from(p.to))),
            })
            .collect();
        if jobs.is_empty() {
            return Err(Error::Other("no files need moving".into()));
        }
        let n = jobs.len();
        let description = if n == 1 {
            "Rename 1 file".to_owned()
        } else {
            format!("Rename {n} files")
        };
        self.run(jobs, &description, None, false)
    }

    /// Moves one file (and a .lrc beside it), creating folders and removing ones left empty.
    fn move_file(
        &self,
        batch: i64,
        track_id: i64,
        from: &std::path::Path,
        to: &std::path::Path,
    ) -> std::result::Result<(), (String, String)> {
        let fail = |e: String| (from.display().to_string(), e);
        if !from.exists() {
            return Err(fail("the file isn't there any more; left as it is".into()));
        }
        if to.exists() {
            return Err(fail(format!("{} already exists", to.display())));
        }
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| fail(e.to_string()))?;
        }
        std::fs::rename(from, to).map_err(|e| fail(e.to_string()))?;
        let lrc_from = from.with_extension("lrc");
        let lrc_to = to.with_extension("lrc");
        if lrc_from.exists() && !lrc_to.exists() {
            let _ = std::fs::rename(&lrc_from, &lrc_to);
        }
        let roots: Vec<PathBuf> = self
            .library
            .roots()
            .unwrap_or_default()
            .into_iter()
            .map(|r| PathBuf::from(r.path))
            .collect();
        let mut dir = from.parent();
        while let Some(d) = dir {
            if roots.iter().any(|r| r == d) || std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
        self.library
            .record_edit(
                batch,
                track_id,
                vec![(
                    FILE.to_owned(),
                    vec![from.display().to_string()],
                    vec![to.display().to_string()],
                )],
            )
            .map_err(|e| fail(format!("moved, but not recorded for undo: {e}")))?;
        Ok(())
    }

    /// Reads an image file into the artwork cache, for use in a picture edit.
    pub(crate) fn import(&self, path: &std::path::Path) -> Result<cadenza_library::StoredArt> {
        let meta = std::fs::metadata(path).map_err(|e| Error::Other(e.to_string()))?;
        if meta.len() >= 16 << 20 {
            return Err(Error::Other("the image must be smaller than 16 MB".into()));
        }
        let bytes = std::fs::read(path).map_err(|e| Error::Other(e.to_string()))?;
        self.library.store_artwork(&bytes)?.ok_or_else(|| {
            Error::Other("that file isn't a complete JPEG, PNG, WebP, GIF or BMP image".into())
        })
    }

    /// Writes a track's front cover beside it, as cover.jpg (or .png...), unless an image of that
    /// name is already there. Returns the new file's path.
    pub(crate) fn save_cover(&self, track_id: i64) -> Result<PathBuf> {
        let track = self
            .library
            .playable(&[track_id])?
            .into_iter()
            .next()
            .ok_or_else(|| Error::Other("the song is no longer in the library".into()))?;
        let tags = cadenza_tags::read(&track.path).map_err(|e| Error::Other(e.to_string()))?;
        let cover = tags
            .front_cover()
            .ok_or_else(|| Error::Other("this song has no artwork".into()))?;
        let ext = if cover.data.starts_with(&[0x89, b'P', b'N', b'G']) {
            "png"
        } else {
            "jpg"
        };
        let dir = track
            .path
            .parent()
            .ok_or_else(|| Error::Other("the song has no folder".into()))?;
        let dest = dir.join(format!("cover.{ext}"));
        if dest.exists() {
            return Err(Error::Other(format!("{} already exists", dest.display())));
        }
        std::fs::write(&dest, &cover.data).map_err(|e| Error::Other(e.to_string()))?;
        Ok(dest)
    }
}

fn parse_picture(s: &str) -> Option<PictureRef> {
    let mut parts = s.splitn(3, ':');
    let picture_type = parts.next()?.parse().ok()?;
    let hash = parts.next()?;
    if hash == "?" {
        return None;
    }
    Some(PictureRef {
        picture_type,
        hash: hash.to_owned(),
        description: parts.next().unwrap_or_default().to_owned(),
    })
}

fn format_name(format: cadenza_tags::TagFormat) -> &'static str {
    use cadenza_tags::TagFormat as F;
    match format {
        F::Id3v22 => "id3v2.2",
        F::Id3v23 => "id3v2.3",
        F::Id3v24 => "id3v2.4",
        F::Vorbis => "vorbis",
        F::Mp4 => "mp4",
        F::Apev2 => "apev2",
        F::Id3v1 => "id3v1",
        F::None => "none",
    }
}
