//! Incremental, parallel library scanning with move detection.
//!
//! 1. Walk the root and `stat` every file (cheap; this is all an unchanged library costs).
//! 2. Compare with the catalogue by (size, mtime, inode): only new or changed files are read.
//! 3. Read tags, hash audio and store artwork in parallel.
//! 4. Write in batched transactions. A new file whose audio hash matches a track that vanished
//!    in this scan is a move: the existing row (id, uid, stats, playlists) follows the file.
//! 5. Tracks still unaccounted for are marked missing (hidden, recoverable if they return).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rayon::prelude::*;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::albums::{self, AlbumIdentity};
use crate::art::{ArtCache, StoredArt, folder_image};
use crate::cue::{self, CueSheet};
use crate::db::{Db, now};
use crate::text::{leading_number, sort_key, year};
use crate::{Error, Result};

const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "m4a", "m4b", "mp4", "aac", "alac", "ogg", "oga", "opus", "wav", "wave", "aif",
    "aiff", "aifc", "wv", "ape", "dsf", "dff", "caf",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Walking,
    Reading,
    Writing,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: ScanPhase,
    pub found: u64,
    pub to_read: u64,
    pub read: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub added: u64,
    pub updated: u64,
    pub moved: u64,
    pub missing: u64,
    pub unchanged: u64,
    pub failed: u64,
    /// First errors encountered (path, message), capped.
    pub errors: Vec<(String, String)>,
    pub elapsed_ms: u64,
}

struct FileStat {
    rel: String,
    abs: PathBuf,
    size: i64,
    mtime: i64,
    inode: i64,
}

struct Existing {
    id: i64,
    size: i64,
    mtime: i64,
    inode: i64,
    audio_hash: String,
    missing: bool,
}

/// Everything needed to insert or update one track row.
#[derive(Debug, Clone)]
pub(crate) struct NewTrack {
    pub rel: String,
    pub cue_index: i64,
    pub start_frame: Option<i64>,
    pub end_frame: Option<i64>,
    pub size: i64,
    pub mtime: i64,
    pub inode: i64,
    pub audio_hash: String,
    pub audio: cadenza_tags::AudioInfo,
    pub fields: cadenza_tags::Fields,
    pub art: Option<StoredArt>,
    pub legacy_encoding: Option<String>,
    pub folder: String,
}

fn walk(
    root: &Path,
    out: &mut Vec<FileStat>,
    cues: &mut Vec<FileStat>,
    rel: &str,
    found: &AtomicU64,
) {
    let Ok(entries) = fs::read_dir(root.join(rel)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let child = if rel.is_empty() {
            name.to_owned()
        } else {
            format!("{rel}/{name}")
        };
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk(root, out, cues, &child, found);
            continue;
        }
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let stat = FileStat {
            abs: root.join(&child),
            rel: child,
            size: meta.len() as i64,
            mtime: meta.mtime(),
            inode: meta.ino() as i64,
        };
        if ext == "cue" {
            cues.push(stat);
        } else if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
            found.fetch_add(1, Ordering::Relaxed);
            out.push(stat);
        }
    }
}

fn parent_rel(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
}

fn file_stem(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel)
        .to_owned()
}

/// Reads one audio file into one or more tracks (several when a CUE sheet splits it).
fn read_file(
    f: &FileStat,
    sheet: Option<&CueSheet>,
    cue_mtime: i64,
    art: &ArtCache,
    folder_art: &FolderArt,
) -> Result<Vec<NewTrack>> {
    let tags = cadenza_tags::read(&f.abs).map_err(|e| Error::Tags(e.to_string()))?;
    let audio_hash = crate::hash::audio_hash(&f.abs, tags.audio.container)?;
    let stored = match tags.front_cover() {
        Some(p) => art.store(&p.data)?,
        None => None,
    }
    .or_else(|| folder_art.get(&f.abs, art));
    let legacy_encoding = tags.legacy.first().map(|l| l.encoding.to_owned());
    let folder = parent_rel(&f.rel).to_owned();
    let mtime = f.mtime.max(cue_mtime);

    let embedded;
    let sheet = match sheet {
        Some(s) => Some(s),
        None => match tags.cuesheet.as_deref() {
            Some(text) => {
                embedded = cue::parse(text);
                (!embedded.tracks.is_empty()).then_some(&embedded)
            }
            None => None,
        },
    };

    let base = NewTrack {
        rel: f.rel.clone(),
        cue_index: 0,
        start_frame: None,
        end_frame: None,
        size: f.size,
        mtime,
        inode: f.inode,
        audio_hash,
        audio: tags.audio.clone(),
        fields: tags.fields.clone(),
        art: stored,
        legacy_encoding,
        folder,
    };
    let Some(sheet) = sheet else {
        let mut t = base;
        if !t.fields.contains_key("title") {
            t.fields.insert("title".into(), vec![file_stem(&f.rel)]);
        }
        return Ok(vec![t]);
    };

    // CUE: one track per sheet entry; frames from CD frames (1/75 s).
    let rate = tags.audio.sample_rate as u64;
    let total_frames = tags.audio.duration_ms * rate / 1000;
    let mut out = Vec::new();
    for (i, ct) in sheet.tracks.iter().enumerate() {
        let start = ct.start_cd_frames * rate / 75;
        let end = sheet
            .tracks
            .get(i + 1)
            .map(|n| n.start_cd_frames * rate / 75);
        let mut t = base.clone();
        t.cue_index = ct.number.max(1) as i64;
        t.audio_hash = format!("{}:{}", base.audio_hash, t.cue_index);
        t.start_frame = Some(start as i64);
        t.end_frame = end.map(|e| e as i64);
        t.audio.duration_ms =
            (end.unwrap_or(total_frames).saturating_sub(start)) * 1000 / rate.max(1);
        let put = |fields: &mut cadenza_tags::Fields, k: &str, v: &Option<String>| {
            if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
                fields.insert(k.into(), vec![v.clone()]);
            }
        };
        t.fields.insert(
            "title".into(),
            vec![
                ct.title
                    .clone()
                    .unwrap_or_else(|| format!("Track {}", ct.number)),
            ],
        );
        put(
            &mut t.fields,
            "artist",
            &ct.performer.clone().or(sheet.performer.clone()),
        );
        put(&mut t.fields, "albumartist", &sheet.performer);
        put(&mut t.fields, "album", &sheet.title);
        put(&mut t.fields, "date", &sheet.date);
        put(&mut t.fields, "genre", &sheet.genre);
        put(&mut t.fields, "isrc", &ct.isrc);
        t.fields
            .insert("tracknumber".into(), vec![ct.number.to_string()]);
        t.fields
            .insert("tracktotal".into(), vec![sheet.tracks.len().to_string()]);
        out.push(t);
    }
    Ok(out)
}

/// Folder images are looked up once per directory.
#[derive(Default)]
struct FolderArt {
    cache: std::sync::Mutex<HashMap<PathBuf, Option<StoredArt>>>,
}

impl FolderArt {
    fn get(&self, file: &Path, art: &ArtCache) -> Option<StoredArt> {
        let dir = file.parent()?.to_path_buf();
        if let Some(hit) = self.cache.lock().ok()?.get(&dir) {
            return hit.clone();
        }
        let stored = folder_image(&dir)
            .and_then(|p| fs::read(p).ok())
            .and_then(|b| art.store(&b).ok().flatten());
        self.cache.lock().ok()?.insert(dir, stored.clone());
        stored
    }
}

pub(crate) fn scan_root(
    db: &Db,
    art: &ArtCache,
    root_id: i64,
    root: &Path,
    progress: &(dyn Fn(ScanProgress) + Sync),
) -> Result<ScanReport> {
    let started = Instant::now();
    let found = AtomicU64::new(0);
    progress(ScanProgress {
        phase: ScanPhase::Walking,
        found: 0,
        to_read: 0,
        read: 0,
    });
    let mut files = Vec::new();
    let mut cue_files = Vec::new();
    walk(root, &mut files, &mut cue_files, "", &found);

    // Sidecar CUE sheets, keyed by the audio file they describe.
    let mut sheets: HashMap<String, (CueSheet, i64)> = HashMap::new();
    for c in &cue_files {
        let Ok(bytes) = fs::read(&c.abs) else {
            continue;
        };
        let sheet = cue::parse(&cue::decode_text(&bytes));
        if let Some(file) = sheet.file.clone().filter(|_| !sheet.tracks.is_empty()) {
            let dir = parent_rel(&c.rel);
            let audio_rel = if dir.is_empty() {
                file
            } else {
                format!("{dir}/{file}")
            };
            sheets.insert(audio_rel, (sheet, c.mtime));
        }
    }

    let existing: HashMap<(String, i64), Existing> = db.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, rel_path, cue_index, size, mtime, inode, audio_hash, missing_since IS NOT NULL FROM tracks WHERE root_id = ?",
        )?;
        let rows = stmt.query_map([root_id], |r| {
            Ok((
                (r.get::<_, String>(1)?, r.get::<_, i64>(2)?),
                Existing { id: r.get(0)?, size: r.get(3)?, mtime: r.get(4)?, inode: r.get(5)?, audio_hash: r.get(6)?, missing: r.get(7)? },
            ))
        })?;
        Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
    })?;
    let by_rel: HashMap<&str, Vec<&Existing>> =
        existing
            .iter()
            .fold(HashMap::new(), |mut m, ((rel, _), e)| {
                m.entry(rel.as_str()).or_default().push(e);
                m
            });

    let mut report = ScanReport::default();
    let mut to_read = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();
    for f in &files {
        let cue_mtime = sheets.get(&f.rel).map(|(_, m)| *m).unwrap_or(0);
        let rows = by_rel.get(f.rel.as_str());
        let unchanged = rows.is_some_and(|rows| {
            rows.iter().all(|e| {
                !e.missing
                    && e.size == f.size
                    && e.mtime == f.mtime.max(cue_mtime)
                    && e.inode == f.inode
            })
        });
        if unchanged {
            report.unchanged += rows.map(|r| r.len() as u64).unwrap_or(0);
            seen.extend(rows.into_iter().flatten().map(|e| e.id));
        } else {
            to_read.push((f, cue_mtime));
        }
    }

    let total = to_read.len() as u64;
    let read_count = AtomicU64::new(0);
    progress(ScanProgress {
        phase: ScanPhase::Reading,
        found: files.len() as u64,
        to_read: total,
        read: 0,
    });
    let folder_art = FolderArt::default();
    let results: Vec<(String, Result<Vec<NewTrack>>)> = to_read
        .par_iter()
        .map(|(f, cue_mtime)| {
            let r = read_file(
                f,
                sheets.get(&f.rel).map(|(s, _)| s),
                *cue_mtime,
                art,
                &folder_art,
            );
            let n = read_count.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(200) || n == total {
                progress(ScanProgress {
                    phase: ScanPhase::Reading,
                    found: files.len() as u64,
                    to_read: total,
                    read: n,
                });
            }
            (f.rel.clone(), r)
        })
        .collect();

    let mut new_tracks = Vec::new();
    for (rel, r) in results {
        match r {
            Ok(tracks) => new_tracks.extend(tracks),
            Err(e) => {
                report.failed += 1;
                if report.errors.len() < 100 {
                    report.errors.push((rel, e.to_string()));
                }
            }
        }
    }

    progress(ScanProgress {
        phase: ScanPhase::Writing,
        found: files.len() as u64,
        to_read: total,
        read: total,
    });
    // Rows whose file vanished: candidates for move detection, else missing.
    let on_disk: HashSet<(String, i64)> = new_tracks
        .iter()
        .map(|t| (t.rel.clone(), t.cue_index))
        .collect();
    let mut gone: HashMap<String, Vec<i64>> = HashMap::new();
    for ((rel, cue), e) in &existing {
        if !seen.contains(&e.id) && !on_disk.contains(&(rel.clone(), *cue)) {
            gone.entry(e.audio_hash.clone()).or_default().push(e.id);
        }
    }
    let existing_ids: Arc<HashMap<(String, i64), i64>> =
        Arc::new(existing.iter().map(|(k, e)| (k.clone(), e.id)).collect());
    let gone = Arc::new(Mutex::new(gone));

    for batch in new_tracks.chunks(500) {
        let batch = batch.to_vec();
        let existing_ids = Arc::clone(&existing_ids);
        let gone = Arc::clone(&gone);
        let outcome = db.write(move |tx| {
            let mut outcome = (0u64, 0u64, 0u64);
            let mut touched = HashSet::new();
            for t in &batch {
                let target = existing_ids.get(&(t.rel.clone(), t.cue_index)).copied();
                let moved_from = match target {
                    None => gone
                        .lock()
                        .map_err(|_| Error::Closed)?
                        .get_mut(&t.audio_hash)
                        .and_then(|ids| ids.pop()),
                    Some(_) => None,
                };
                let id = match (target, moved_from) {
                    (Some(id), _) => {
                        outcome.1 += 1;
                        id
                    }
                    (None, Some(id)) => {
                        outcome.2 += 1;
                        id
                    }
                    (None, None) => {
                        outcome.0 += 1;
                        insert_stub(tx, root_id, t)?
                    }
                };
                if let Some(old_album) = tx
                    .query_row("SELECT album_id FROM tracks WHERE id = ?", [id], |r| {
                        r.get::<_, Option<i64>>(0)
                    })
                    .optional()?
                    .flatten()
                {
                    touched.insert(old_album);
                }
                touched.insert(write_track(tx, id, t)?);
            }
            albums::refresh(tx, &touched)?;
            Ok(outcome)
        })?;
        report.added += outcome.0;
        report.updated += outcome.1;
        report.moved += outcome.2;
    }

    let missing: Vec<i64> = std::mem::take(&mut *gone.lock().map_err(|_| Error::Closed)?)
        .into_values()
        .flatten()
        .collect();
    report.missing = missing.len() as u64;
    if !missing.is_empty() {
        db.write(move |tx| {
            let mut touched = HashSet::new();
            for id in &missing {
                let album: Option<i64> =
                    tx.query_row("SELECT album_id FROM tracks WHERE id = ?", [id], |r| {
                        r.get(0)
                    })?;
                touched.extend(album);
                tx.execute(
                    "UPDATE tracks SET missing_since = COALESCE(missing_since, ?) WHERE id = ?",
                    params![now(), id],
                )?;
                tx.execute("DELETE FROM search WHERE rowid = ?", [id])?;
            }
            albums::refresh(tx, &touched)?;
            Ok(())
        })?;
    }
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    progress(ScanProgress {
        phase: ScanPhase::Done,
        found: files.len() as u64,
        to_read: total,
        read: total,
    });
    Ok(report)
}

/// Creates a minimal row (stable uid, added date) that `write_track` then fills.
fn insert_stub(tx: &Transaction<'_>, root_id: i64, t: &NewTrack) -> Result<i64> {
    tx.execute(
        "INSERT INTO tracks (uid, audio_hash, root_id, rel_path, cue_index, size, mtime, inode, container, codec,
            lossless, sample_rate, channels, duration_ms, bitrate, quality, title, artist, album, albumartist,
            composer, genre, sort_title, sort_artist, sort_album, added_at, scanned_at)
         VALUES (?, ?, ?, ?, ?, 0, 0, 0, '', '', 0, 0, 0, 0, 0, '', '', '', '', '', '', '', '', '', '', ?, ?)",
        params![uuid::Uuid::now_v7().to_string(), t.audio_hash, root_id, t.rel, t.cue_index, now(), now()],
    )?;
    let id = tx.last_insert_rowid();
    // Ratings and play counts stored in the file seed the library on first sight.
    let rating = t
        .fields
        .get("rating")
        .and_then(|v| v.first())
        .and_then(|r| r.parse::<i64>().ok());
    let plays = t
        .fields
        .get("playcount")
        .and_then(|v| v.first())
        .and_then(|r| r.parse::<i64>().ok())
        .unwrap_or(0);
    if rating.is_some() || plays > 0 {
        tx.execute(
            "INSERT OR IGNORE INTO user_data (track_uid, rating, play_count) SELECT uid, ?, ? FROM tracks WHERE id = ?",
            params![rating, plays, id],
        )?;
    }
    Ok(id)
}

/// ReplayGain from tags, gains in dB at the ReplayGain 2 reference (−18 LUFS). Opus R128 gains
/// (Q7.8 dB at −23 LUFS) are converted. `None` without a track gain.
pub(crate) fn tag_gain(f: &cadenza_tags::Fields) -> Option<crate::Gain> {
    let number = |k: &str| {
        let v = first(f, k)?.trim();
        let end = v
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '.')))
            .unwrap_or(v.len());
        v[..end].parse::<f64>().ok().filter(|x| x.is_finite())
    };
    let r128 = |k: &str| number(k).map(|q| q / 256.0 + 5.0);
    let track = number("replaygain_track_gain").or_else(|| r128("custom:R128_TRACK_GAIN"))?;
    Some(crate::Gain {
        track_gain: Some(track),
        track_peak: number("replaygain_track_peak"),
        album_gain: number("replaygain_album_gain").or_else(|| r128("custom:R128_ALBUM_GAIN")),
        album_peak: number("replaygain_album_peak"),
    })
}

fn first<'a>(fields: &'a cadenza_tags::Fields, k: &str) -> Option<&'a str> {
    fields.get(k).and_then(|v| v.first()).map(String::as_str)
}

/// "A", "A & B", "A, B & C".
pub(crate) fn join_names(values: &[String]) -> String {
    match values {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} & {last}", init.join(", ")),
    }
}

pub(crate) fn quality(a: &cadenza_tags::AudioInfo) -> &'static str {
    if a.dsd_rate.is_some() {
        "dsd"
    } else if !a.codec.is_lossless() {
        "lossy"
    } else if a.bit_depth.unwrap_or(16) > 16 || a.sample_rate > 48_000 {
        "hires"
    } else {
        "lossless"
    }
}

/// Fills a track row from `t`, rewrites its tags and search entry; returns its album id.
fn write_track(tx: &Transaction<'_>, id: i64, t: &NewTrack) -> Result<i64> {
    let f = &t.fields;
    let get = |k: &str| f.get(k).map(|v| join_names(v)).unwrap_or_default();
    let title = get("title");
    let artist = get("artist");
    let album = get("album");
    let compilation = first(f, "compilation") == Some("1");
    let albumartist = match f.get("albumartist") {
        Some(v) => join_names(v),
        None if compilation => "Various Artists".to_owned(),
        None => artist.clone(),
    };
    let year = first(f, "date").or(first(f, "originaldate")).and_then(year);
    let identity = AlbumIdentity {
        title: album.clone(),
        albumartist: albumartist.clone(),
        release_mbid: first(f, "musicbrainz_albumid").map(str::to_owned),
        folder: t.folder.clone(),
    };
    let album_id = albums::upsert(tx, &identity, f, t.art.as_ref())?;
    if let Some(a) = &t.art {
        tx.execute(
            "INSERT OR IGNORE INTO artwork (hash, mime, width, height, bytes) VALUES (?, ?, ?, ?, ?)",
            params![a.hash, a.mime, a.width, a.height, a.bytes as i64],
        )?;
    }
    let a = &t.audio;
    tx.execute(
        "UPDATE tracks SET audio_hash = ?, rel_path = ?, cue_index = ?, start_frame = ?, end_frame = ?, size = ?, mtime = ?,
            inode = ?, container = ?, codec = ?, lossless = ?, sample_rate = ?, bit_depth = ?, float = ?, channels = ?,
            dsd_rate = ?, duration_ms = ?, bitrate = ?, quality = ?, title = ?, artist = ?, album = ?, albumartist = ?,
            composer = ?, genre = ?, year = ?, track_no = ?, track_total = ?, disc_no = ?, disc_total = ?, sort_title = ?,
            sort_artist = ?, sort_album = ?, compilation = ?, album_id = ?, art_hash = ?, legacy_encoding = ?,
            scanned_at = ?, missing_since = NULL
         WHERE id = ?",
        params![
            t.audio_hash,
            t.rel,
            t.cue_index,
            t.start_frame,
            t.end_frame,
            t.size,
            t.mtime,
            t.inode,
            a.container,
            a.codec.as_str(),
            a.codec.is_lossless(),
            a.sample_rate,
            a.bit_depth,
            a.float,
            a.channels,
            a.dsd_rate,
            a.duration_ms as i64,
            a.bitrate_kbps,
            quality(a),
            title,
            artist,
            album,
            albumartist,
            get("composer"),
            f.get("genre").map(|v| v.join("; ")).unwrap_or_default(),
            year,
            first(f, "tracknumber").and_then(leading_number),
            first(f, "tracktotal").and_then(leading_number),
            first(f, "discnumber").and_then(leading_number),
            first(f, "disctotal").and_then(leading_number),
            sort_key(first(f, "titlesort").unwrap_or(&title)),
            sort_key(first(f, "artistsort").unwrap_or(&artist)),
            sort_key(first(f, "albumsort").unwrap_or(&album)),
            compilation,
            album_id,
            t.art.as_ref().map(|a| a.hash.clone()),
            t.legacy_encoding,
            now(),
            id,
        ],
    )?;
    // ReplayGain: the file's tags win; Cadenza's own measurement survives a rescan only while the
    // audio is unchanged (`audio_hash` above still holds the old value until this statement).
    match tag_gain(f) {
        Some(g) => tx.execute(
            "UPDATE tracks SET rg_track_gain = ?, rg_track_peak = ?, rg_album_gain = ?,
                rg_album_peak = ?, rg_source = 'tags' WHERE id = ?",
            params![g.track_gain, g.track_peak, g.album_gain, g.album_peak, id],
        )?,
        None => tx.execute(
            "UPDATE tracks SET rg_track_gain = NULL, rg_track_peak = NULL, rg_album_gain = NULL,
                rg_album_peak = NULL, rg_source = NULL
             WHERE id = ? AND NOT (rg_source IS 'analysis' AND rg_hash IS ?)",
            params![id, t.audio_hash],
        )?,
    };
    tx.execute("DELETE FROM tags WHERE track_id = ?", [id])?;
    {
        let mut ins = tx
            .prepare_cached("INSERT INTO tags (track_id, field, idx, value) VALUES (?, ?, ?, ?)")?;
        for (field, values) in f {
            for (i, v) in values.iter().enumerate() {
                ins.execute(params![id, field, i as i64, v])?;
            }
        }
    }
    let extra: Vec<String> = ["work", "conductor", "label", "grouping", "artists", "date"]
        .iter()
        .filter_map(|k| f.get(*k).map(|v| v.join(" ")))
        .chain(
            f.iter()
                .filter(|(k, _)| k.starts_with("performer"))
                .map(|(_, v)| v.join(" ")),
        )
        .collect();
    tx.execute("DELETE FROM search WHERE rowid = ?", [id])?;
    tx.execute(
        "INSERT INTO search (rowid, title, artist, album, albumartist, composer, genre, extra) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![id, title, artist, album, albumartist, get("composer"), f.get("genre").map(|v| v.join(" ")).unwrap_or_default(), extra.join(" ")],
    )?;
    Ok(album_id)
}
