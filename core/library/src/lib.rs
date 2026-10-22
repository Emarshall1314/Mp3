//! The library catalogue: a SQLite replica of what is on disk, kept current by an incremental
//! scanner, with artwork, albums, artists and search on top.

mod albums;
mod art;
mod cue;
mod db;
mod hash;
mod history;
mod lyrics;
mod pages;
mod scan;
mod search;
mod text;

use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Row, params, params_from_iter};
use serde::Serialize;

pub use art::{StoredArt, THUMB_SIZES};
pub use history::{EditBatch, EditChange};
pub use lyrics::{LyricLine, Lyrics};
pub use pages::{ArtistPage, Home, ReleaseSection};
pub use scan::{ScanPhase, ScanProgress, ScanReport};

use art::ArtCache;
use db::{Db, now};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("database schema: {0}")]
    Schema(String),
    #[error("tag read failed: {0}")]
    Tags(String),
    #[error("image error: {0}")]
    Image(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("the library is closed")]
    Closed,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub id: i64,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    pub id: i64,
    pub uid: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub albumartist: String,
    pub album_id: Option<i64>,
    pub duration_ms: i64,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: String,
    pub codec: String,
    pub sample_rate: i64,
    pub bit_depth: Option<i64>,
    pub dsd_rate: Option<i64>,
    pub bitrate: i64,
    pub quality: String,
    pub art_hash: Option<String>,
    pub favourite: bool,
    pub play_count: i64,
}

const TRACK_COLUMNS: &str = "t.id, t.uid, t.title, t.artist, t.album, t.albumartist, t.album_id, t.duration_ms,
    t.track_no, t.disc_no, t.year, t.genre, t.codec, t.sample_rate, t.bit_depth, t.dsd_rate, t.bitrate, t.quality,
    t.art_hash, COALESCE(u.favourite, 0), COALESCE(u.play_count, 0)";
const TRACK_FROM: &str = "tracks t LEFT JOIN user_data u ON u.track_uid = t.uid";

fn track_row(r: &Row<'_>) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
        id: r.get(0)?,
        uid: r.get(1)?,
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        albumartist: r.get(5)?,
        album_id: r.get(6)?,
        duration_ms: r.get(7)?,
        track_no: r.get(8)?,
        disc_no: r.get(9)?,
        year: r.get(10)?,
        genre: r.get(11)?,
        codec: r.get(12)?,
        sample_rate: r.get(13)?,
        bit_depth: r.get(14)?,
        dsd_rate: r.get(15)?,
        bitrate: r.get(16)?,
        quality: r.get(17)?,
        art_hash: r.get(18)?,
        favourite: r.get(19)?,
        play_count: r.get(20)?,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRow {
    pub id: i64,
    pub title: String,
    pub albumartist: String,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub track_count: i64,
    pub duration_ms: i64,
    pub quality: Option<String>,
    pub art_hash: Option<String>,
    pub compilation: bool,
    pub added_at: i64,
}

const ALBUM_COLUMNS: &str = "a.id, a.title, a.albumartist, a.year, a.genre, a.track_count, a.duration_ms, a.quality, a.art_hash, a.compilation, a.added_at";

fn album_row(r: &Row<'_>) -> rusqlite::Result<AlbumRow> {
    Ok(AlbumRow {
        id: r.get(0)?,
        title: r.get(1)?,
        albumartist: r.get(2)?,
        year: r.get(3)?,
        genre: r.get(4)?,
        track_count: r.get(5)?,
        duration_ms: r.get(6)?,
        quality: r.get(7)?,
        art_hash: r.get(8)?,
        compilation: r.get(9)?,
        added_at: r.get(10)?,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDetail {
    pub album: AlbumRow,
    pub date: Option<String>,
    pub label: Option<String>,
    pub disc_count: i64,
    pub tracks: Vec<TrackRow>,
    pub other_versions: Vec<AlbumRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRow {
    pub name: String,
    pub album_count: i64,
    pub art_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SongSort {
    Title,
    Artist,
    Album,
    Year,
    Duration,
    Added,
    Genre,
    Plays,
}

impl SongSort {
    fn order_by(self, desc: bool) -> String {
        let d = if desc { " DESC" } else { "" };
        match self {
            SongSort::Title => format!("t.sort_title{d}, t.sort_artist, t.id"),
            SongSort::Artist => {
                format!("t.sort_artist{d}, t.sort_album{d}, t.disc_no, t.track_no, t.id")
            }
            SongSort::Album => format!("t.sort_album{d}, t.disc_no, t.track_no, t.id"),
            SongSort::Year => {
                format!("t.year{d}, t.sort_artist, t.sort_album, t.disc_no, t.track_no, t.id")
            }
            SongSort::Duration => format!("t.duration_ms{d}, t.id"),
            SongSort::Added => format!("t.added_at{d}, t.id{d}"),
            SongSort::Genre => format!("t.genre{d}, t.sort_artist, t.id"),
            SongSort::Plays => format!("COALESCE(u.play_count, 0){d}, t.sort_artist, t.id"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlbumSort {
    Artist,
    Title,
    Year,
    Added,
}

/// What the player needs to open a track.
const PLAYABLE_COLUMNS: &str = "t.id, t.uid, r.path, t.rel_path, t.start_frame, t.end_frame, \
    t.album_id, t.rg_track_gain, t.rg_track_peak, t.rg_album_gain, t.rg_album_peak";

fn playable_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<PlayableTrack> {
    let root: String = r.get(2)?;
    let rel: String = r.get(3)?;
    Ok(PlayableTrack {
        id: r.get(0)?,
        uid: r.get(1)?,
        path: Path::new(&root).join(rel),
        start_frame: r.get::<_, Option<i64>>(4)?.map(|v| v as u64),
        end_frame: r.get::<_, Option<i64>>(5)?.map(|v| v as u64),
        album_id: r.get(6)?,
        gain: Gain {
            track_gain: r.get(7)?,
            track_peak: r.get(8)?,
            album_gain: r.get(9)?,
            album_peak: r.get(10)?,
        },
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayableTrack {
    pub id: i64,
    pub uid: String,
    pub path: PathBuf,
    pub start_frame: Option<u64>,
    pub end_frame: Option<u64>,
    pub album_id: Option<i64>,
    pub gain: Gain,
}

/// ReplayGain values (dB at −18 LUFS; peaks as linear sample peaks).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gain {
    pub track_gain: Option<f64>,
    pub track_peak: Option<f64>,
    pub album_gain: Option<f64>,
    pub album_peak: Option<f64>,
}

/// A track to measure: what to decode, and what the result belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct GainTarget {
    pub track: PlayableTrack,
    pub audio_hash: String,
}

/// A measured track, ready to store.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasuredGain {
    pub track_id: i64,
    pub gain: Gain,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub songs: Vec<TrackRow>,
    /// Matching songs, counted up to [`SEARCH_COUNT_CAP`].
    pub song_count: i64,
    /// True when there are more matches than `song_count` says.
    pub song_count_capped: bool,
    pub albums: Vec<AlbumRow>,
    pub artists: Vec<ArtistRow>,
}

/// Searches count matches up to this many; beyond it the UI says "10,000+".
pub const SEARCH_COUNT_CAP: i64 = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSearch {
    pub id: i64,
    pub name: String,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub tracks: i64,
    pub albums: i64,
    pub artists: i64,
    pub duration_ms: i64,
}

#[derive(Debug)]
pub struct Library {
    db: Db,
    art: ArtCache,
}

impl Library {
    /// Opens (creating if needed) a library in `data_dir`: `library.db` plus an `artwork/` cache.
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db = Db::open(&data_dir.join("library.db"))?;
        Ok(Self {
            db,
            art: ArtCache::new(data_dir.join("artwork")),
        })
    }

    pub fn roots(&self) -> Result<Vec<Root>> {
        self.db.read(|c| {
            let mut s = c.prepare("SELECT id, path FROM roots ORDER BY path")?;
            let rows = s.query_map([], |r| {
                Ok(Root {
                    id: r.get(0)?,
                    path: r.get(1)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn add_root(&self, path: &Path) -> Result<Root> {
        let path = path.canonicalize()?;
        if !path.is_dir() {
            return Err(Error::NotFound(format!("folder {}", path.display())));
        }
        let p = path.to_string_lossy().into_owned();
        self.db.write(move |tx| {
            tx.execute(
                "INSERT OR IGNORE INTO roots (path, added_at) VALUES (?, ?)",
                params![p, now()],
            )?;
            let id = tx.query_row("SELECT id FROM roots WHERE path = ?", [&p], |r| r.get(0))?;
            Ok(Root { id, path: p })
        })
    }

    /// Removes a folder from the library (files are never touched).
    pub fn remove_root(&self, id: i64) -> Result<()> {
        self.db.write(move |tx| {
            tx.execute("DELETE FROM search WHERE rowid IN (SELECT id FROM tracks WHERE root_id = ?)", [id])?;
            tx.execute("DELETE FROM roots WHERE id = ?", [id])?;
            tx.execute("DELETE FROM albums WHERE id NOT IN (SELECT DISTINCT album_id FROM tracks WHERE album_id IS NOT NULL)", [])?;
            Ok(())
        })
    }

    /// Scans every root incrementally. Blocking: run it on a background thread.
    pub fn scan(&self, progress: &(dyn Fn(ScanProgress) + Sync)) -> Result<ScanReport> {
        let mut total = ScanReport::default();
        for root in self.roots()? {
            let r = scan::scan_root(
                &self.db,
                &self.art,
                root.id,
                Path::new(&root.path),
                progress,
            )?;
            total.added += r.added;
            total.updated += r.updated;
            total.moved += r.moved;
            total.missing += r.missing;
            total.unchanged += r.unchanged;
            total.failed += r.failed;
            total.errors.extend(r.errors);
            total.elapsed_ms += r.elapsed_ms;
        }
        Ok(total)
    }

    pub fn stats(&self) -> Result<LibraryStats> {
        self.db.read(|c| {
            Ok(c.query_row(
                "SELECT (SELECT COUNT(*) FROM tracks WHERE missing_since IS NULL),
                        (SELECT COUNT(*) FROM albums),
                        (SELECT COUNT(DISTINCT albumartist) FROM albums),
                        (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks WHERE missing_since IS NULL)",
                [],
                |r| Ok(LibraryStats { tracks: r.get(0)?, albums: r.get(1)?, artists: r.get(2)?, duration_ms: r.get(3)? }),
            )?)
        })
    }

    pub fn songs(
        &self,
        sort: SongSort,
        desc: bool,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TrackRow>> {
        // Page through ids on the (covering, partial) sort index first; only the page's rows are
        // then read in full. A deep OFFSET over full rows costs a table lookup per skipped row.
        let from = match sort {
            SongSort::Plays => TRACK_FROM,
            _ => "tracks t",
        };
        let sql = format!(
            "SELECT t.id FROM {from} WHERE t.missing_since IS NULL ORDER BY {} LIMIT ? OFFSET ?",
            sort.order_by(desc)
        );
        let ids: Vec<i64> = self.db.read(|c| {
            let mut s = c.prepare_cached(&sql)?;
            let rows = s.query_map(params![limit, offset], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        self.tracks(&ids)
    }

    /// All song ids in the given order (for "play all" and shuffle from the songs list).
    pub fn song_ids(&self, sort: SongSort, desc: bool) -> Result<Vec<i64>> {
        let sql = format!(
            "SELECT t.id FROM {TRACK_FROM} WHERE t.missing_since IS NULL ORDER BY {}",
            sort.order_by(desc)
        );
        self.db.read(|c| {
            let mut s = c.prepare_cached(&sql)?;
            let rows = s.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn tracks(&self, ids: &[i64]) -> Result<Vec<TrackRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(",");
        let sql = format!("SELECT {TRACK_COLUMNS} FROM {TRACK_FROM} WHERE t.id IN ({marks})");
        let mut rows: Vec<TrackRow> = self.db.read(|c| {
            let mut s = c.prepare(&sql)?;
            let rows = s.query_map(params_from_iter(ids.iter()), track_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        // Preserve the caller's order.
        let pos: std::collections::HashMap<i64, usize> =
            ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        rows.sort_by_key(|r| pos.get(&r.id).copied().unwrap_or(usize::MAX));
        Ok(rows)
    }

    pub fn albums(&self, sort: AlbumSort, offset: i64, limit: i64) -> Result<Vec<AlbumRow>> {
        let order = match sort {
            AlbumSort::Artist => "a.sort_artist, a.year, a.sort_title",
            AlbumSort::Title => "a.sort_title, a.sort_artist",
            AlbumSort::Year => "a.year DESC, a.sort_artist, a.sort_title",
            AlbumSort::Added => "a.added_at DESC, a.id DESC",
        };
        let sql = format!("SELECT {ALBUM_COLUMNS} FROM albums a ORDER BY {order} LIMIT ? OFFSET ?");
        self.db.read(|c| {
            let mut s = c.prepare_cached(&sql)?;
            let rows = s.query_map(params![limit, offset], album_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn album(&self, id: i64) -> Result<AlbumDetail> {
        self.db.read(|c| {
            let (album, date, label, disc_count, identity): (AlbumRow, Option<String>, Option<String>, i64, String) = c
                .query_row(
                    &format!("SELECT {ALBUM_COLUMNS}, a.date, a.label, a.disc_count, a.identity_key FROM albums a WHERE a.id = ?"),
                    [id],
                    |r| Ok((album_row(r)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?)),
                )
                .optional()?
                .ok_or_else(|| Error::NotFound(format!("album {id}")))?;
            let mut s = c.prepare_cached(&format!(
                "SELECT {TRACK_COLUMNS} FROM {TRACK_FROM} WHERE t.album_id = ? AND t.missing_since IS NULL
                 ORDER BY COALESCE(t.disc_no, 1), t.track_no, t.sort_title"
            ))?;
            let tracks = s.query_map([id], track_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut s = c.prepare_cached(&format!(
                "SELECT {ALBUM_COLUMNS} FROM albums a WHERE a.identity_key = ? AND a.id != ? ORDER BY a.year"
            ))?;
            let other_versions = s.query_map(params![identity, id], album_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(AlbumDetail { album, date, label, disc_count, tracks, other_versions })
        })
    }

    /// Album artists (as Apple Music lists artists), with album counts.
    pub fn artists(&self) -> Result<Vec<ArtistRow>> {
        self.db.read(|c| {
            let mut s = c.prepare_cached(
                "SELECT albumartist, COUNT(*), MAX(art_hash) FROM albums GROUP BY albumartist COLLATE NOCASE
                 ORDER BY MIN(sort_artist)",
            )?;
            let rows = s.query_map([], |r| Ok(ArtistRow { name: r.get(0)?, album_count: r.get(1)?, art_hash: r.get(2)? }))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn artist_albums(&self, name: &str) -> Result<Vec<AlbumRow>> {
        let name = name.to_owned();
        self.db.read(|c| {
            let mut s = c.prepare_cached(&format!(
                "SELECT {ALBUM_COLUMNS} FROM albums a WHERE a.albumartist = ? COLLATE NOCASE ORDER BY a.year, a.sort_title"
            ))?;
            let rows = s.query_map([name], album_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn search(&self, query: &str, limit: i64) -> Result<SearchResults> {
        let compiled = search::compile(query, now());
        let mut params = compiled.params.clone();
        let (from, order, where_extra) = match &compiled.fts_match {
            Some(m) => {
                params.insert(0, rusqlite::types::Value::Text(m.clone()));
                (
                    "search s JOIN tracks t ON t.id = s.rowid LEFT JOIN user_data u ON u.track_uid = t.uid".to_owned(),
                    "s.rank",
                    "search MATCH ? AND ",
                )
            }
            None => (
                TRACK_FROM.to_owned(),
                "t.sort_artist, t.sort_album, t.disc_no, t.track_no",
                "",
            ),
        };
        let where_sql = format!("{where_extra}{}", compiled.where_sql);
        let count_sql = format!(
            "SELECT COUNT(*) FROM (SELECT 1 FROM {from} WHERE {where_sql} LIMIT {})",
            SEARCH_COUNT_CAP + 1
        );
        let plain: Vec<String> = query
            .split_whitespace()
            .filter(|w| !w.contains(':') && !w.starts_with('-'))
            .map(|w| w.trim_matches('"').to_owned())
            .filter(|w| !w.is_empty())
            .collect();
        self.db.read(|c| {
            // Count first (cheap, capped), then pick the plan. Ranking costs ~1 µs per match, so
            // very broad text queries list by artist instead. Without text, few matches mean a
            // selective filter: `+` stops SQLite walking the sort index past all non-matches.
            let counted: i64 = c.query_row(&count_sql, params_from_iter(params.iter()), |r| r.get(0))?;
            let (song_count, song_count_capped) = (counted.min(SEARCH_COUNT_CAP), counted > SEARCH_COUNT_CAP);
            let by_artist = "t.sort_artist, t.sort_album, t.disc_no, t.track_no";
            let order = match (compiled.fts_match.is_some(), counted) {
                (true, n) if n <= 5_000 => order.to_owned(),
                (true, _) => by_artist.to_owned(),
                (false, n) if n <= 2_000 => format!("+{by_artist}"),
                (false, _) => by_artist.to_owned(),
            };
            let songs_sql = format!("SELECT {TRACK_COLUMNS} FROM {from} WHERE {where_sql} ORDER BY {order} LIMIT {limit}");
            let songs = c
                .prepare(&songs_sql)?
                .query_map(params_from_iter(params.iter()), track_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let (mut albums, mut artists) = (Vec::new(), Vec::new());
            if !plain.is_empty() {
                let cond = vec!["(a.title || ' ' || a.albumartist) LIKE ?"; plain.len()].join(" AND ");
                let likes: Vec<String> = plain.iter().map(|w| format!("%{w}%")).collect();
                albums = c
                    .prepare(&format!("SELECT {ALBUM_COLUMNS} FROM albums a WHERE {cond} ORDER BY a.sort_artist, a.year LIMIT 24"))?
                    .query_map(params_from_iter(likes.iter()), album_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let cond = vec!["albumartist LIKE ?"; plain.len()].join(" AND ");
                artists = c
                    .prepare(&format!(
                        "SELECT albumartist, COUNT(*), MAX(art_hash) FROM albums WHERE {cond}
                         GROUP BY albumartist COLLATE NOCASE ORDER BY MIN(sort_artist) LIMIT 12"
                    ))?
                    .query_map(params_from_iter(likes.iter()), |r| {
                        Ok(ArtistRow { name: r.get(0)?, album_count: r.get(1)?, art_hash: r.get(2)? })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
            }
            Ok(SearchResults { songs, song_count, song_count_capped, albums, artists })
        })
    }

    pub fn saved_searches(&self) -> Result<Vec<SavedSearch>> {
        self.db.read(|c| {
            let mut s = c.prepare("SELECT id, name, query FROM saved_searches ORDER BY name")?;
            let rows = s.query_map([], |r| {
                Ok(SavedSearch {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    query: r.get(2)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn save_search(&self, name: &str, query: &str) -> Result<SavedSearch> {
        let (name, query) = (name.to_owned(), query.to_owned());
        self.db.write(move |tx| {
            tx.execute(
                "INSERT INTO saved_searches (name, query, created_at) VALUES (?, ?, ?)",
                params![name, query, now()],
            )?;
            Ok(SavedSearch {
                id: tx.last_insert_rowid(),
                name,
                query,
            })
        })
    }

    pub fn delete_saved_search(&self, id: i64) -> Result<()> {
        self.db.write(move |tx| {
            tx.execute("DELETE FROM saved_searches WHERE id = ?", [id])?;
            Ok(())
        })
    }

    /// Resolves tracks to files for the player.
    /// Present tracks without ReplayGain, album by album, for loudness measurement.
    pub fn tracks_missing_gain(&self) -> Result<Vec<GainTarget>> {
        let sql = format!(
            "SELECT {PLAYABLE_COLUMNS}, t.audio_hash
             FROM tracks t JOIN roots r ON r.id = t.root_id
             WHERE t.rg_track_gain IS NULL AND t.missing_since IS NULL
             ORDER BY t.album_id, t.disc_no, t.track_no, t.id"
        );
        self.db.read(move |c| {
            let mut s = c.prepare(&sql)?;
            let rows = s.query_map([], |r| {
                Ok(GainTarget {
                    track: playable_row(r)?,
                    audio_hash: r.get(11)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Stores loudness measured by Cadenza. Tags read later replace it; it is dropped if the audio
    /// changes.
    pub fn store_measured_gain(&self, measured: Vec<MeasuredGain>) -> Result<()> {
        self.db.write(move |tx| {
            let mut up = tx.prepare_cached(
                "UPDATE tracks SET rg_track_gain = ?, rg_track_peak = ?, rg_album_gain = ?,
                    rg_album_peak = ?, rg_source = 'analysis', rg_hash = audio_hash
                 WHERE id = ? AND rg_track_gain IS NULL",
            )?;
            for m in &measured {
                up.execute(params![
                    m.gain.track_gain,
                    m.gain.track_peak,
                    m.gain.album_gain,
                    m.gain.album_peak,
                    m.track_id
                ])?;
            }
            Ok(())
        })
    }

    pub fn playable(&self, ids: &[i64]) -> Result<Vec<PlayableTrack>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(",");
        let sql = format!(
            "SELECT {PLAYABLE_COLUMNS}
             FROM tracks t JOIN roots r ON r.id = t.root_id WHERE t.id IN ({marks}) AND t.missing_since IS NULL"
        );
        let found: Vec<PlayableTrack> = self.db.read(|c| {
            let mut s = c.prepare(&sql)?;
            let rows = s.query_map(params_from_iter(ids.iter()), playable_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        let pos: std::collections::HashMap<i64, usize> =
            ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let mut found = found;
        found.sort_by_key(|t| pos.get(&t.id).copied().unwrap_or(usize::MAX));
        Ok(found)
    }

    /// Records a finished (or substantially played) listen.
    pub fn record_play(&self, track_id: i64, played_ms: i64, completed: bool) -> Result<()> {
        self.db.write(move |tx| {
            let uid: String = tx.query_row("SELECT uid FROM tracks WHERE id = ?", [track_id], |r| r.get(0))?;
            tx.execute(
                "INSERT INTO play_events (track_uid, at, played_ms, completed) VALUES (?, ?, ?, ?)",
                params![uid, now(), played_ms, completed],
            )?;
            if completed {
                tx.execute(
                    "INSERT INTO user_data (track_uid, play_count, last_played) VALUES (?, 1, ?)
                     ON CONFLICT(track_uid) DO UPDATE SET play_count = play_count + 1, last_played = excluded.last_played",
                    params![uid, now()],
                )?;
            }
            Ok(())
        })
    }

    /// A thumbnail file for an artwork hash, at least `size` px (generated on first request).
    pub fn artwork_thumbnail(&self, hash: &str, size: u32) -> Result<PathBuf> {
        self.art.thumbnail(hash, size)
    }

    /// Dominant colours of an artwork (up to four, most prominent first), from its 128 px
    /// thumbnail.
    pub fn artwork_palette(&self, hash: &str) -> Result<Vec<[u8; 3]>> {
        art::palette(&self.art.thumbnail(hash, 128)?)
    }

    /// Puts image bytes in the artwork cache (by content hash); `None` if they aren't a complete,
    /// readable image.
    pub fn store_artwork(&self, bytes: &[u8]) -> Result<Option<StoredArt>> {
        self.art.store(bytes)
    }

    /// The full-resolution original for an artwork hash.
    pub fn artwork_original(&self, hash: &str) -> Result<PathBuf> {
        self.art
            .original(hash)
            .ok_or_else(|| Error::NotFound(format!("artwork {hash}")))
    }

    /// The track's lyrics: a sidecar `.lrc` beside the file, else embedded synced lyrics, else
    /// embedded plain lyrics. `None` if there are none.
    pub fn lyrics(&self, track_id: i64) -> Result<Option<Lyrics>> {
        let (path, whole_file, synced, plain) = self.db.read(move |c| {
            let (root, rel, start): (String, String, Option<i64>) = c.query_row(
                "SELECT r.path, t.rel_path, t.start_frame FROM tracks t JOIN roots r ON r.id = t.root_id
                 WHERE t.id = ?",
                [track_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let tag = |field: &str| -> rusqlite::Result<Option<String>> {
                c.query_row(
                    "SELECT value FROM tags WHERE track_id = ? AND field = ? ORDER BY idx LIMIT 1",
                    params![track_id, field],
                    |r| r.get(0),
                )
                .optional()
            };
            Ok((
                Path::new(&root).join(rel),
                start.is_none(),
                tag("syncedlyrics")?,
                tag("lyrics")?,
            ))
        })?;
        // A sidecar belongs to a whole file, not to one track of a cue-split album.
        if whole_file {
            for ext in ["lrc", "LRC"] {
                if let Ok(text) = std::fs::read_to_string(path.with_extension(ext))
                    && let Some(l) = lyrics::parse(&text, "file")
                {
                    return Ok(Some(l));
                }
            }
        }
        Ok(synced
            .and_then(|t| lyrics::parse(&t, "embedded"))
            .or_else(|| plain.and_then(|t| lyrics::parse(&t, "embedded"))))
    }
}
