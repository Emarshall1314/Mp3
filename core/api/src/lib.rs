//! `cadenza-api`: the only crate app shells depend on.
//!
//! One [`Cadenza`] handle owns the library and the player, keeps the catalogue in step with the
//! disk (background scans, a file-system watcher, periodic verification) and counts plays.
//! Types stay plain so the same API can later be exported through UniFFI if a native mobile UI
//! is needed.

mod watch;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Serialize;

pub use cadenza_engine::{Event as PlayerEvent, PlayerState, Repeat, Status, StreamInfo};
pub use cadenza_library::{
    AlbumDetail, AlbumRow, AlbumSort, ArtistRow, LibraryStats, Root, SavedSearch, ScanPhase,
    ScanProgress, ScanReport, SearchResults, SongSort, TrackRow,
};

use cadenza_engine::{Engine, OutputBackend, QueueItem};
use cadenza_library::Library;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Library(#[from] cadenza_library::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Facts about the running build, shown in About and attached to bug reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub name: String,
    pub version: String,
    pub target_os: String,
    pub target_arch: String,
    pub debug: bool,
}

pub fn build_info() -> BuildInfo {
    BuildInfo {
        name: "Cadenza".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target_os: std::env::consts::OS.to_owned(),
        target_arch: std::env::consts::ARCH.to_owned(),
        debug: cfg!(debug_assertions),
    }
}

/// Everything the UI is told about without asking.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ApiEvent {
    Player {
        event: PlayerEvent,
    },
    ScanProgress {
        progress: ScanProgress,
    },
    ScanFinished {
        report: ScanReport,
    },
    /// The catalogue changed; lists should refresh.
    LibraryChanged,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub running: bool,
    pub progress: Option<ScanProgress>,
    pub last_report: Option<ScanReport>,
}

type Emit = Arc<dyn Fn(ApiEvent) + Send + Sync>;

struct Scanner {
    library: Arc<Library>,
    status: Mutex<ScanStatus>,
    running: AtomicBool,
    again: AtomicBool,
    emit: Emit,
}

impl Scanner {
    /// Starts a background scan, or schedules one more pass if a scan is already running.
    fn request(self: &Arc<Self>) {
        if self.running.swap(true, Ordering::AcqRel) {
            self.again.store(true, Ordering::Release);
            return;
        }
        let me = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("cadenza-scan".into())
            .spawn(move || {
                loop {
                    me.again.store(false, Ordering::Release);
                    if let Ok(mut s) = me.status.lock() {
                        s.running = true;
                    }
                    let progress_target = Arc::clone(&me);
                    let result = me.library.scan(&move |p: ScanProgress| {
                        if let Ok(mut s) = progress_target.status.lock() {
                            s.progress = Some(p.clone());
                        }
                        (progress_target.emit)(ApiEvent::ScanProgress { progress: p });
                    });
                    let report = match result {
                        Ok(r) => r,
                        Err(e) => ScanReport {
                            errors: vec![(String::new(), e.to_string())],
                            failed: 1,
                            ..Default::default()
                        },
                    };
                    let changed = report.added + report.updated + report.moved + report.missing > 0;
                    if let Ok(mut s) = me.status.lock() {
                        s.running = false;
                        s.last_report = Some(report.clone());
                    }
                    (me.emit)(ApiEvent::ScanFinished { report });
                    if changed {
                        (me.emit)(ApiEvent::LibraryChanged);
                    }
                    if !me.again.load(Ordering::Acquire) {
                        break;
                    }
                }
                me.running.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            self.running.store(false, Ordering::Release);
        }
    }
}

pub struct Cadenza {
    library: Arc<Library>,
    engine: Engine,
    scanner: Arc<Scanner>,
    _watcher: Mutex<Option<watch::Watcher>>,
}

impl std::fmt::Debug for Cadenza {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cadenza").finish_non_exhaustive()
    }
}

/// The platform's default output.
pub fn default_output() -> Box<dyn OutputBackend> {
    #[cfg(target_os = "linux")]
    {
        let device = std::env::var("CADENZA_ALSA_DEVICE").unwrap_or_else(|_| "default".into());
        Box::new(cadenza_engine::output::alsa::Alsa { device })
    }
    #[cfg(target_os = "android")]
    {
        Box::new(cadenza_engine::output::aaudio::AAudio)
    }
    // Linux and Android are the only platforms (D11); elsewhere the core builds for tests only.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        Box::new(cadenza_engine::Capture::new())
    }
}

impl Cadenza {
    /// Opens the library in `data_dir` and starts the player on `output`.
    pub fn open(
        data_dir: &Path,
        output: Box<dyn OutputBackend>,
        on_event: impl Fn(ApiEvent) + Send + Sync + 'static,
    ) -> Result<Self> {
        let library = Arc::new(Library::open(data_dir)?);
        let emit: Emit = Arc::new(on_event);
        let player_emit = Arc::clone(&emit);
        let counting = Arc::clone(&library);
        let engine = Engine::new(output, move |event| {
            if let PlayerEvent::TrackFinished {
                track_id,
                played_ms,
                completed,
            } = &event
            {
                let _ = counting.record_play(*track_id, *played_ms as i64, *completed);
            }
            player_emit(ApiEvent::Player { event });
        });
        let scanner = Arc::new(Scanner {
            library: Arc::clone(&library),
            status: Mutex::new(ScanStatus::default()),
            running: AtomicBool::new(false),
            again: AtomicBool::new(false),
            emit,
        });
        let me = Self {
            library,
            engine,
            scanner,
            _watcher: Mutex::new(None),
        };
        me.restart_watcher();
        Ok(me)
    }

    fn restart_watcher(&self) {
        let roots: Vec<PathBuf> = self
            .library
            .roots()
            .unwrap_or_default()
            .into_iter()
            .map(|r| PathBuf::from(r.path))
            .collect();
        let scanner = Arc::clone(&self.scanner);
        let w = watch::Watcher::start(roots, move || scanner.request());
        if let Ok(mut slot) = self._watcher.lock() {
            *slot = w;
        }
    }

    // ---- library ------------------------------------------------------------------------

    pub fn roots(&self) -> Result<Vec<Root>> {
        Ok(self.library.roots()?)
    }

    /// Adds a music folder and scans it.
    pub fn add_root(&self, path: &Path) -> Result<Root> {
        let root = self.library.add_root(path)?;
        self.restart_watcher();
        self.scanner.request();
        Ok(root)
    }

    pub fn remove_root(&self, id: i64) -> Result<()> {
        self.library.remove_root(id)?;
        self.restart_watcher();
        (self.scanner.emit)(ApiEvent::LibraryChanged);
        Ok(())
    }

    pub fn rescan(&self) {
        self.scanner.request();
    }

    pub fn scan_status(&self) -> ScanStatus {
        self.scanner
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn stats(&self) -> Result<LibraryStats> {
        Ok(self.library.stats()?)
    }

    pub fn songs(
        &self,
        sort: SongSort,
        desc: bool,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TrackRow>> {
        Ok(self.library.songs(sort, desc, offset, limit)?)
    }

    pub fn tracks(&self, ids: &[i64]) -> Result<Vec<TrackRow>> {
        Ok(self.library.tracks(ids)?)
    }

    pub fn albums(&self, sort: AlbumSort, offset: i64, limit: i64) -> Result<Vec<AlbumRow>> {
        Ok(self.library.albums(sort, offset, limit)?)
    }

    pub fn album(&self, id: i64) -> Result<AlbumDetail> {
        Ok(self.library.album(id)?)
    }

    pub fn artists(&self) -> Result<Vec<ArtistRow>> {
        Ok(self.library.artists()?)
    }

    pub fn artist_albums(&self, name: &str) -> Result<Vec<AlbumRow>> {
        Ok(self.library.artist_albums(name)?)
    }

    pub fn search(&self, query: &str, limit: i64) -> Result<SearchResults> {
        Ok(self.library.search(query, limit)?)
    }

    pub fn saved_searches(&self) -> Result<Vec<SavedSearch>> {
        Ok(self.library.saved_searches()?)
    }

    pub fn save_search(&self, name: &str, query: &str) -> Result<SavedSearch> {
        Ok(self.library.save_search(name, query)?)
    }

    pub fn delete_saved_search(&self, id: i64) -> Result<()> {
        Ok(self.library.delete_saved_search(id)?)
    }

    /// Path of a JPEG thumbnail at least `size` px (generated on first request).
    pub fn artwork(&self, hash: &str, size: u32) -> Result<PathBuf> {
        Ok(self.library.artwork_thumbnail(hash, size)?)
    }

    // ---- playback -----------------------------------------------------------------------

    fn items(&self, ids: &[i64]) -> Result<Vec<QueueItem>> {
        Ok(self
            .library
            .playable(ids)?
            .into_iter()
            .map(|p| QueueItem {
                track_id: p.id,
                path: p.path,
                start_frame: p.start_frame,
                end_frame: p.end_frame,
            })
            .collect())
    }

    /// Replaces the queue with `ids` and plays from `start`.
    pub fn play_tracks(&self, ids: &[i64], start: usize, shuffle: bool) -> Result<()> {
        let items = self.items(ids)?;
        if items.is_empty() {
            return Err(Error::Other("nothing playable".into()));
        }
        let start = if shuffle {
            0
        } else {
            start.min(items.len() - 1)
        };
        self.engine.set_shuffle(shuffle);
        self.engine.set_queue(items, start, true);
        Ok(())
    }

    pub fn play_album(&self, album_id: i64, start: usize, shuffle: bool) -> Result<()> {
        let ids: Vec<i64> = self
            .library
            .album(album_id)?
            .tracks
            .iter()
            .map(|t| t.id)
            .collect();
        self.play_tracks(&ids, start, shuffle)
    }

    /// Plays the whole Songs list in its current order, starting at `index`.
    pub fn play_songs(
        &self,
        sort: SongSort,
        desc: bool,
        index: usize,
        shuffle: bool,
    ) -> Result<()> {
        let ids = self.library.song_ids(sort, desc)?;
        self.play_tracks(&ids, index, shuffle)
    }

    pub fn play_next(&self, ids: &[i64]) -> Result<()> {
        self.engine.insert_next(self.items(ids)?);
        Ok(())
    }

    pub fn play_later(&self, ids: &[i64]) -> Result<()> {
        self.engine.append(self.items(ids)?);
        Ok(())
    }

    pub fn player(&self) -> PlayerState {
        self.engine.state()
    }

    /// Queue entries `offset..offset + limit` as track rows, in play order.
    pub fn queue_window(&self, offset: usize, limit: usize) -> Result<Vec<TrackRow>> {
        let ids = self.engine.queue_ids(offset, limit);
        Ok(self.library.tracks(&ids)?)
    }

    pub fn toggle(&self) {
        self.engine.toggle();
    }
    pub fn play(&self) {
        self.engine.play();
    }
    pub fn pause(&self) {
        self.engine.pause();
    }
    pub fn stop(&self) {
        self.engine.stop();
    }
    pub fn next(&self) {
        self.engine.next();
    }
    pub fn previous(&self) {
        self.engine.previous();
    }
    pub fn play_index(&self, index: usize) {
        self.engine.play_index(index);
    }
    pub fn seek(&self, position_ms: u64) {
        self.engine.seek(position_ms);
    }
    pub fn set_volume(&self, volume: f32) {
        self.engine.set_volume(volume);
    }
    pub fn remove_from_queue(&self, index: usize) {
        self.engine.remove(index);
    }
    pub fn move_in_queue(&self, from: usize, to: usize) {
        self.engine.move_item(from, to);
    }
    pub fn clear_upcoming(&self) {
        self.engine.clear_upcoming();
    }
    pub fn set_repeat(&self, repeat: Repeat) {
        self.engine.set_repeat(repeat);
    }
    pub fn set_shuffle(&self, on: bool) {
        self.engine.set_shuffle(on);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_info_reports_this_crate_version() {
        let info = build_info();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(info.target_os, std::env::consts::OS);
    }
}
