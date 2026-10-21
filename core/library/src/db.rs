//! SQLite access: one writer thread owning the only write connection, plus a small pool of
//! read-only connections. WAL mode lets reads proceed while a scan writes, so the UI never waits
//! on the scanner.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use crossbeam_channel::{Sender, bounded, unbounded};
use rusqlite::{Connection, OpenFlags, Transaction};

use crate::{Error, Result};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// Schema migrations, applied in order. Never edit a released step; append a new one.
const MIGRATIONS: &[&str] = &[
    // 1: initial catalogue
    r#"
    CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE roots(
        id INTEGER PRIMARY KEY,
        path TEXT NOT NULL UNIQUE,
        added_at INTEGER NOT NULL
    );
    CREATE TABLE albums(
        id INTEGER PRIMARY KEY,
        key TEXT NOT NULL UNIQUE,
        title TEXT NOT NULL,
        albumartist TEXT NOT NULL,
        sort_title TEXT NOT NULL,
        sort_artist TEXT NOT NULL,
        identity_key TEXT NOT NULL,
        year INTEGER,
        date TEXT,
        genre TEXT,
        label TEXT,
        compilation INTEGER NOT NULL DEFAULT 0,
        release_mbid TEXT,
        release_group_mbid TEXT,
        folder TEXT,
        track_count INTEGER NOT NULL DEFAULT 0,
        disc_count INTEGER NOT NULL DEFAULT 1,
        duration_ms INTEGER NOT NULL DEFAULT 0,
        quality TEXT,
        art_hash TEXT,
        added_at INTEGER NOT NULL
    );
    CREATE INDEX albums_sort_artist ON albums(sort_artist, year, sort_title);
    CREATE INDEX albums_sort_title ON albums(sort_title);
    CREATE INDEX albums_added ON albums(added_at DESC);
    CREATE INDEX albums_identity ON albums(identity_key);
    CREATE TABLE tracks(
        id INTEGER PRIMARY KEY,
        uid TEXT NOT NULL UNIQUE,
        audio_hash TEXT NOT NULL,
        root_id INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
        rel_path TEXT NOT NULL,
        cue_index INTEGER NOT NULL DEFAULT 0,
        start_frame INTEGER,
        end_frame INTEGER,
        size INTEGER NOT NULL,
        mtime INTEGER NOT NULL,
        inode INTEGER NOT NULL,
        container TEXT NOT NULL,
        codec TEXT NOT NULL,
        lossless INTEGER NOT NULL,
        sample_rate INTEGER NOT NULL,
        bit_depth INTEGER,
        float INTEGER NOT NULL DEFAULT 0,
        channels INTEGER NOT NULL,
        dsd_rate INTEGER,
        duration_ms INTEGER NOT NULL,
        bitrate INTEGER NOT NULL,
        quality TEXT NOT NULL,
        title TEXT NOT NULL,
        artist TEXT NOT NULL,
        album TEXT NOT NULL,
        albumartist TEXT NOT NULL,
        composer TEXT NOT NULL,
        genre TEXT NOT NULL,
        year INTEGER,
        track_no INTEGER,
        track_total INTEGER,
        disc_no INTEGER,
        disc_total INTEGER,
        sort_title TEXT NOT NULL,
        sort_artist TEXT NOT NULL,
        sort_album TEXT NOT NULL,
        compilation INTEGER NOT NULL DEFAULT 0,
        album_id INTEGER REFERENCES albums(id) ON DELETE SET NULL,
        art_hash TEXT,
        legacy_encoding TEXT,
        added_at INTEGER NOT NULL,
        scanned_at INTEGER NOT NULL,
        missing_since INTEGER,
        UNIQUE(root_id, rel_path, cue_index)
    );
    CREATE INDEX tracks_hash ON tracks(audio_hash);
    CREATE INDEX tracks_album ON tracks(album_id, disc_no, track_no);
    -- Partial indexes over present tracks: every list query filters on missing_since IS NULL,
    -- so these cover paging and sorting without touching the table.
    CREATE INDEX tracks_sort_artist ON tracks(sort_artist, sort_album, disc_no, track_no) WHERE missing_since IS NULL;
    CREATE INDEX tracks_sort_title ON tracks(sort_title, sort_artist) WHERE missing_since IS NULL;
    CREATE INDEX tracks_sort_album ON tracks(sort_album, disc_no, track_no) WHERE missing_since IS NULL;
    CREATE INDEX tracks_added ON tracks(added_at DESC) WHERE missing_since IS NULL;
    CREATE INDEX tracks_year ON tracks(year) WHERE missing_since IS NULL;
    CREATE INDEX tracks_codec ON tracks(codec) WHERE missing_since IS NULL;
    CREATE INDEX tracks_container ON tracks(container) WHERE missing_since IS NULL;
    CREATE INDEX tracks_quality ON tracks(quality) WHERE missing_since IS NULL;
    CREATE INDEX tracks_rate ON tracks(sample_rate) WHERE missing_since IS NULL;
    CREATE INDEX tracks_bits ON tracks(bit_depth) WHERE missing_since IS NULL;
    CREATE INDEX tracks_duration ON tracks(duration_ms) WHERE missing_since IS NULL;
    CREATE INDEX tracks_genre ON tracks(genre, sort_artist) WHERE missing_since IS NULL;
    CREATE TABLE tags(
        track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
        field TEXT NOT NULL,
        idx INTEGER NOT NULL,
        value TEXT NOT NULL,
        PRIMARY KEY(track_id, field, idx)
    ) WITHOUT ROWID;
    CREATE INDEX tags_field ON tags(field, value);
    CREATE TABLE artwork(
        hash TEXT PRIMARY KEY,
        mime TEXT NOT NULL,
        width INTEGER NOT NULL,
        height INTEGER NOT NULL,
        bytes INTEGER NOT NULL
    );
    CREATE TABLE user_data(
        track_uid TEXT PRIMARY KEY,
        rating INTEGER,
        favourite INTEGER NOT NULL DEFAULT 0,
        play_count INTEGER NOT NULL DEFAULT 0,
        skip_count INTEGER NOT NULL DEFAULT 0,
        last_played INTEGER
    );
    CREATE TABLE play_events(
        id INTEGER PRIMARY KEY,
        track_uid TEXT NOT NULL,
        at INTEGER NOT NULL,
        played_ms INTEGER NOT NULL,
        completed INTEGER NOT NULL
    );
    CREATE INDEX play_events_track ON play_events(track_uid, at);
    CREATE TABLE saved_searches(
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        query TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE VIRTUAL TABLE search USING fts5(
        title, artist, album, albumartist, composer, genre, extra,
        content='', contentless_delete=1, tokenize='unicode61 remove_diacritics 2', prefix='1 2 3'
    );
    "#,
];

pub(crate) struct Db {
    writer: Sender<Job>,
    readers: Arc<Mutex<Vec<Connection>>>,
    path: PathBuf,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").field("path", &self.path).finish()
    }
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA cache_size = -32000;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(())
}

impl Db {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        // WAL survives crashes without corrupting the database and lets readers run alongside
        // the writer.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        configure(&conn)?;
        migrate(&mut conn)?;

        let (tx, rx) = unbounded::<Job>();
        thread::Builder::new()
            .name("cadenza-db-writer".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job(&mut conn);
                }
            })
            .map_err(Error::Io)?;

        let mut readers = Vec::new();
        for _ in 0..4 {
            let r = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | OpenFlags::SQLITE_OPEN_URI,
            )?;
            configure(&r)?;
            readers.push(r);
        }
        Ok(Self {
            writer: tx,
            readers: Arc::new(Mutex::new(readers)),
            path: path.to_owned(),
        })
    }

    /// Runs `f` in a transaction on the writer thread and waits for the result.
    pub(crate) fn write<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<T> + Send + 'static,
    {
        let (done_tx, done_rx) = bounded(1);
        let job: Job = Box::new(move |conn: &mut Connection| {
            let result = (|| {
                let tx = conn.transaction()?;
                let value = f(&tx)?;
                tx.commit()?;
                Ok(value)
            })();
            let _ = done_tx.send(result);
        });
        self.writer.send(job).map_err(|_| Error::Closed)?;
        done_rx.recv().map_err(|_| Error::Closed)?
    }

    /// Runs `f` with a pooled read-only connection (opens an extra one if all are busy).
    pub(crate) fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.readers.lock().map_err(|_| Error::Closed)?.pop();
        let conn = match conn {
            Some(c) => c,
            None => {
                let c = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                configure(&c)?;
                c
            }
        };
        let result = f(&conn);
        if let Ok(mut pool) = self.readers.lock() {
            pool.push(conn);
        }
        result
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
        tx.commit()?;
    }
    if version as usize > MIGRATIONS.len() {
        return Err(Error::Schema(format!(
            "database schema v{version} is newer than this build (v{})",
            MIGRATIONS.len()
        )));
    }
    Ok(())
}

pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
