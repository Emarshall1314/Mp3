//! What sync needs from the library: songs by audio hash, favourites, listens that can be
//! imported once, and the trash.
//!
//! The trash is a folder in the data folder. A trashed song's file (and its `.lrc`) moves into
//! `trash/<id>/`, and the song is hidden as if missing, so restoring it (moving the files back and
//! scanning) brings back the same song with its history. Trashed files are deleted after 30 days.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::Serialize;

use crate::db::now;
use crate::{Error, Library, Result, albums};

/// How long the trash keeps files.
pub const TRASH_DAYS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    pub id: i64,
    pub audio_hash: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Where it was, to go back to.
    pub original_path: String,
    /// Seconds since the Unix epoch.
    pub trashed_at: i64,
}

/// A song's file, for sharing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackFile {
    pub id: i64,
    pub audio_hash: String,
    /// Relative to its music folder.
    pub rel_path: String,
    pub size: u64,
    pub path: PathBuf,
}

/// A listen recorded before sync was turned on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsyncedPlay {
    pub id: i64,
    pub audio_hash: String,
    pub at: i64,
    pub played_ms: i64,
    pub completed: bool,
}

/// Moves a file, copying across file systems when a rename can't.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::File::open(to)?.sync_all()?;
            std::fs::remove_file(from)
        }
    }
}

/// The song's `.lrc` beside it, if there is one.
fn sidecar(path: &Path) -> Option<PathBuf> {
    let lrc = path.with_extension("lrc");
    lrc.is_file().then_some(lrc)
}

impl Library {
    fn trash_dir(&self) -> PathBuf {
        self.data_dir.join("trash")
    }

    /// Audio hashes of present songs, by id.
    pub fn audio_hashes(&self, ids: &[i64]) -> Result<Vec<(i64, String)>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(",");
        self.db.read(|c| {
            let mut s = c.prepare(&format!(
                "SELECT id, audio_hash FROM tracks WHERE id IN ({marks}) AND missing_since IS NULL"
            ))?;
            let rows = s.query_map(params_from_iter(ids), |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Present songs with this audio (usually one; more if the same file is in two folders).
    pub fn tracks_with_hash(&self, hash: &str) -> Result<Vec<i64>> {
        let hash = hash.to_owned();
        self.db.read(move |c| {
            let mut s = c.prepare(
                "SELECT id FROM tracks WHERE audio_hash = ? AND missing_since IS NULL ORDER BY id",
            )?;
            let rows = s.query_map([hash], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Present songs with any of these audio hashes: (hash, id) pairs.
    pub fn tracks_with_hashes(&self, hashes: &[String]) -> Result<Vec<(String, i64)>> {
        let mut out = Vec::new();
        for chunk in hashes.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(",");
            let chunk = chunk.to_vec();
            out.extend(self.db.read(move |c| {
                let mut s = c.prepare(&format!(
                    "SELECT audio_hash, id FROM tracks WHERE audio_hash IN ({marks}) AND missing_since IS NULL ORDER BY id"
                ))?;
                let rows = s.query_map(params_from_iter(chunk.iter()), |r| Ok((r.get(0)?, r.get(1)?)))?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })?);
        }
        Ok(out)
    }

    /// Present songs scanned (added or changed) at or after `at` (seconds), with their hashes.
    pub fn scanned_since(&self, at: i64) -> Result<Vec<(i64, String)>> {
        self.db.read(move |c| {
            let mut s = c.prepare(
                "SELECT id, audio_hash FROM tracks WHERE scanned_at >= ? AND missing_since IS NULL",
            )?;
            let rows = s.query_map([at], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Every present song's file (songs from cue sheets share theirs, so they're left out).
    pub fn track_files(&self) -> Result<Vec<TrackFile>> {
        self.db.read(|c| {
            let mut s = c.prepare(
                "SELECT t.id, t.audio_hash, t.rel_path, t.size, r.path FROM tracks t
                 JOIN roots r ON r.id = t.root_id
                 WHERE t.missing_since IS NULL AND t.cue_index = 0 ORDER BY t.id",
            )?;
            let rows = s.query_map([], |r| {
                let root: String = r.get(4)?;
                let rel: String = r.get(2)?;
                Ok(TrackFile {
                    id: r.get(0)?,
                    audio_hash: r.get(1)?,
                    path: Path::new(&root).join(&rel),
                    rel_path: rel,
                    size: r.get::<_, i64>(3)? as u64,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Whether any song, present or not (trashed songs are hidden as missing), has this audio.
    pub fn knows_hash(&self, hash: &str) -> Result<bool> {
        let hash = hash.to_owned();
        self.db.read(move |c| {
            Ok(c.query_row(
                "SELECT 1 FROM tracks WHERE audio_hash = ? LIMIT 1",
                [hash],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
        })
    }

    pub fn set_favourite(&self, ids: &[i64], on: bool) -> Result<()> {
        let ids = ids.to_vec();
        self.db.write(move |tx| {
            for id in ids {
                tx.execute(
                    "INSERT INTO user_data (track_uid, favourite) SELECT uid, ? FROM tracks WHERE id = ?
                     ON CONFLICT(track_uid) DO UPDATE SET favourite = excluded.favourite",
                    params![on, id],
                )?;
            }
            Ok(())
        })
    }

    /// Audio hashes of every favourite song.
    pub fn favourite_hashes(&self) -> Result<Vec<String>> {
        self.db.read(|c| {
            let mut s = c.prepare(
                "SELECT DISTINCT t.audio_hash FROM user_data u JOIN tracks t ON t.uid = u.track_uid
                 WHERE u.favourite = 1",
            )?;
            let rows = s.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Adds a listen from another device, once: true if it was new. `at` is in seconds.
    pub fn import_play(
        &self,
        track_id: i64,
        at: i64,
        played_ms: i64,
        completed: bool,
        sync_id: &str,
    ) -> Result<bool> {
        let sync_id = sync_id.to_owned();
        self.db.write(move |tx| {
            let uid: String =
                tx.query_row("SELECT uid FROM tracks WHERE id = ?", [track_id], |r| {
                    r.get(0)
                })?;
            let added = tx.execute(
                "INSERT OR IGNORE INTO play_events (track_uid, at, played_ms, completed, sync_id)
                 VALUES (?, ?, ?, ?, ?)",
                params![uid, at, played_ms, completed, sync_id],
            )? > 0;
            if added && completed {
                tx.execute(
                    "INSERT INTO user_data (track_uid, play_count, last_played) VALUES (?, 1, ?)
                     ON CONFLICT(track_uid) DO UPDATE SET play_count = play_count + 1,
                       last_played = MAX(COALESCE(last_played, 0), excluded.last_played)",
                    params![uid, at],
                )?;
            }
            Ok(added)
        })
    }

    /// Listens not yet given to sync (made before it was turned on).
    pub fn unsynced_plays(&self) -> Result<Vec<UnsyncedPlay>> {
        self.db.read(|c| {
            let mut s = c.prepare(
                "SELECT p.id, t.audio_hash, p.at, p.played_ms, p.completed FROM play_events p
                 JOIN tracks t ON t.uid = p.track_uid WHERE p.sync_id IS NULL ORDER BY p.id",
            )?;
            let rows = s.query_map([], |r| {
                Ok(UnsyncedPlay {
                    id: r.get(0)?,
                    audio_hash: r.get(1)?,
                    at: r.get(2)?,
                    played_ms: r.get(3)?,
                    completed: r.get(4)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Notes the sync ids given to listens.
    pub fn mark_plays_synced(&self, ids: Vec<(i64, String)>) -> Result<()> {
        self.db.write(move |tx| {
            for (id, sync_id) in ids {
                tx.execute(
                    "UPDATE play_events SET sync_id = ? WHERE id = ?",
                    params![sync_id, id],
                )?;
            }
            Ok(())
        })
    }

    /// Moves songs' files to the trash. Songs from a cue sheet share one file with the rest of
    /// their album, so they can't be trashed on their own.
    pub fn trash(&self, ids: &[i64]) -> Result<Vec<TrashItem>> {
        let mut out = Vec::new();
        for track in self.playable(ids)? {
            if track.start_frame.is_some() {
                return Err(Error::NotFound(
                    "songs from a cue sheet can't be moved to the trash on their own".into(),
                ));
            }
            let row = self.tracks(&[track.id])?;
            let row = row.first().ok_or_else(|| Error::NotFound("song".into()))?;
            let (hash,) = self
                .audio_hashes(&[track.id])?
                .into_iter()
                .map(|(_, h)| (h,))
                .next()
                .ok_or_else(|| Error::NotFound("song".into()))?;
            let (title, artist, album) = (row.title.clone(), row.artist.clone(), row.album.clone());
            let original = track.path.display().to_string();
            let at = now();
            let id = self.db.write({
                let (hash, original, title, artist, album) =
                    (hash.clone(), original.clone(), title.clone(), artist.clone(), album.clone());
                move |tx| {
                    tx.execute(
                        "INSERT INTO trash (audio_hash, original_path, title, artist, album, trashed_at)
                         VALUES (?, ?, ?, ?, ?, ?)",
                        params![hash, original, title, artist, album, at],
                    )?;
                    Ok(tx.last_insert_rowid())
                }
            })?;
            let dir = self.trash_dir().join(id.to_string());
            std::fs::create_dir_all(&dir)?;
            let moved = (|| {
                let name = track
                    .path
                    .file_name()
                    .ok_or_else(|| Error::NotFound(original.clone()))?;
                move_file(&track.path, &dir.join(name))?;
                if let Some(lrc) = sidecar(&track.path)
                    && let Some(n) = lrc.file_name()
                {
                    move_file(&lrc, &dir.join(n))?;
                }
                Ok::<_, Error>(())
            })();
            if let Err(e) = moved {
                let _ = std::fs::remove_dir(&dir);
                self.db.write(move |tx| {
                    tx.execute("DELETE FROM trash WHERE id = ?", [id])?;
                    Ok(())
                })?;
                return Err(e);
            }
            let track_id = track.id;
            self.db.write(move |tx| {
                let album: Option<i64> = tx.query_row(
                    "SELECT album_id FROM tracks WHERE id = ?",
                    [track_id],
                    |r| r.get(0),
                )?;
                tx.execute(
                    "UPDATE tracks SET missing_since = ? WHERE id = ?",
                    params![at, track_id],
                )?;
                tx.execute("DELETE FROM search WHERE rowid = ?", [track_id])?;
                albums::refresh(tx, &album.into_iter().collect::<HashSet<_>>())?;
                Ok(())
            })?;
            out.push(TrashItem {
                id,
                audio_hash: hash,
                title,
                artist,
                album,
                original_path: original,
                trashed_at: at,
            });
        }
        Ok(out)
    }

    pub fn trash_items(&self) -> Result<Vec<TrashItem>> {
        self.db.read(|c| {
            let mut s = c.prepare(
                "SELECT id, audio_hash, title, artist, album, original_path, trashed_at FROM trash
                 ORDER BY trashed_at DESC, id DESC",
            )?;
            let rows = s.query_map([], |r| {
                Ok(TrashItem {
                    id: r.get(0)?,
                    audio_hash: r.get(1)?,
                    title: r.get(2)?,
                    artist: r.get(3)?,
                    album: r.get(4)?,
                    original_path: r.get(5)?,
                    trashed_at: r.get(6)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Trash items holding this audio.
    pub fn trashed_with_hash(&self, hash: &str) -> Result<Vec<i64>> {
        Ok(self
            .trash_items()?
            .into_iter()
            .filter(|t| t.audio_hash == hash)
            .map(|t| t.id)
            .collect())
    }

    /// Moves a trashed song's files back where they were. A scan then brings the song back.
    pub fn restore(&self, id: i64) -> Result<()> {
        let item = self
            .trash_items()?
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| Error::NotFound("that song is no longer in the trash".into()))?;
        let original = PathBuf::from(&item.original_path);
        let to_dir = original
            .parent()
            .ok_or_else(|| Error::NotFound(item.original_path.clone()))?;
        std::fs::create_dir_all(to_dir)?;
        let dir = self.trash_dir().join(id.to_string());
        let files: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        for f in &files {
            if let Some(name) = f.file_name()
                && to_dir.join(name).exists()
            {
                return Err(Error::NotFound(format!(
                    "{} is already there; move it first",
                    to_dir.join(name).display()
                )));
            }
        }
        for f in files {
            if let Some(name) = f.file_name() {
                move_file(&f, &to_dir.join(name))?;
            }
        }
        let _ = std::fs::remove_dir(&dir);
        self.db.write(move |tx| {
            tx.execute("DELETE FROM trash WHERE id = ?", [id])?;
            Ok(())
        })
    }

    /// Deletes a trashed song's files for good.
    pub fn purge(&self, id: i64) -> Result<()> {
        let dir = self.trash_dir().join(id.to_string());
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        self.db.write(move |tx| {
            tx.execute("DELETE FROM trash WHERE id = ?", [id])?;
            Ok(())
        })
    }

    /// Deletes what has been in the trash longer than [`TRASH_DAYS`]; how many.
    pub fn purge_expired(&self) -> Result<usize> {
        let cutoff = now() - TRASH_DAYS * 86_400;
        let old: Vec<i64> = self
            .trash_items()?
            .into_iter()
            .filter(|t| t.trashed_at < cutoff)
            .map(|t| t.id)
            .collect();
        for id in &old {
            self.purge(*id)?;
        }
        Ok(old.len())
    }
}
