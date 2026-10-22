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
}

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
}

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
                    .map(|p| PictureInfo {
                        picture_type: p.picture_type,
                        mime: p.mime.clone(),
                        description: p.description.clone(),
                        bytes: p.data.len(),
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
            .filter(|e| !e.fields.is_empty())
            .map(|e| Job {
                track_id: e.track_id,
                fields: e.fields,
                expect: BTreeMap::new(),
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
            });
            job.fields.insert(change.field.clone(), change.old);
            job.expect.insert(change.field, change.new);
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
        let edit = Edit {
            fields: job.fields.clone(),
            pictures: None,
        };
        let after = cadenza_tags::write(&track.path, &edit, &WriteOptions { backup })
            .map_err(|e| fail(e.to_string()))?;
        let changes: Vec<(String, Vec<String>, Vec<String>)> = job
            .fields
            .keys()
            .map(|field| {
                let old = before.fields.get(field).cloned().unwrap_or_default();
                let new = after.fields.get(field).cloned().unwrap_or_default();
                (field.clone(), old, new)
            })
            .filter(|(_, old, new)| old != new)
            .collect();
        if !changes.is_empty() {
            self.library
                .record_edit(batch, job.track_id, changes)
                .map_err(|e| fail(format!("written, but not recorded for undo: {e}")))?;
        }
        Ok(())
    }
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
