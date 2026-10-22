//! `cadenza-api`: the only crate app shells depend on.
//!
//! One [`Cadenza`] handle owns the library and the player, keeps the catalogue in step with the
//! disk (background scans, a file-system watcher, periodic verification) and counts plays.
//! Types stay plain so the same API can later be exported through UniFFI if a native mobile UI
//! is needed.

mod edit;
mod loudness;
mod organise;
mod settings;
mod themes;
mod watch;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Serialize;

pub use cadenza_engine::dsp::{CrossfeedLevel, EqBand, FilterKind, ImportedEq};
pub use cadenza_engine::{
    DeviceFormat, DspSettings, Event as PlayerEvent, OutputDevice, PlayerState, Repeat,
    ReplayGainMode, ReplayGainSettings, Status, StreamInfo,
};
pub use cadenza_library::{
    AlbumDetail, AlbumRow, AlbumSort, ArtistPage, ArtistRow, Browse, BrowseItem, EditBatch,
    EditChange, Home, LibraryStats, LyricLine, Lyrics, ReleaseSection, Root, SavedSearch,
    ScanPhase, ScanProgress, ScanReport, SearchResults, SongSort, StoredArt, TrackRow,
};
pub use edit::{
    EditFailure, EditReport, EditStatus, LegacyFix, PictureEdit, PictureInfo, PictureRef,
    RenamePlan, TrackEdit, TrackTags,
};
pub use loudness::LoudnessStatus;
pub use settings::{Appearance, Settings, Theme};
pub use themes::SavedTheme;

use cadenza_engine::{Engine, OutputBackend, QueueItem, TrackGain};
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
    /// Loudness measurement progress.
    Loudness {
        status: LoudnessStatus,
    },
    /// Tag writing progress.
    EditProgress {
        status: EditStatus,
    },
    /// A batch of tag changes was written (or stopped).
    EditFinished {
        report: EditReport,
    },
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
    analyzer: Arc<loudness::Analyzer>,
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
                        if me.analyzer.auto.load(Ordering::Acquire) {
                            me.analyzer.start();
                        }
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
    settings: settings::Store,
    themes: themes::Themes,
    current: Mutex<Settings>,
    analyzer: Arc<loudness::Analyzer>,
    scanner: Arc<Scanner>,
    editor: Arc<edit::Editor>,
    _watcher: Mutex<Option<watch::Watcher>>,
}

impl std::fmt::Debug for Cadenza {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cadenza").finish_non_exhaustive()
    }
}

/// The outputs on this machine and which one is in use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outputs {
    pub devices: Vec<OutputDevice>,
    pub current: String,
    /// Devices with DSD over DoP turned on.
    pub dop: Vec<String>,
}

/// The backend for a saved device id. On Linux, `CADENZA_ALSA_DEVICE` overrides it (for running
/// the app against ALSA's `null` device in tests and demos).
fn backend_for(settings: &Settings) -> Box<dyn OutputBackend> {
    let id = settings.output_device.as_str();
    #[cfg(target_os = "linux")]
    if let Ok(device) = std::env::var("CADENZA_ALSA_DEVICE") {
        return Box::new(cadenza_engine::output::alsa::Alsa {
            device,
            exclusive: false,
            dop: false,
        });
    }
    cadenza_engine::output::backend(id, settings.dop(id))
}

impl Cadenza {
    /// Opens the library in `data_dir` and starts the player on `output`, or on the saved output
    /// device when `None`.
    pub fn open(
        data_dir: &Path,
        output: Option<Box<dyn OutputBackend>>,
        on_event: impl Fn(ApiEvent) + Send + Sync + 'static,
    ) -> Result<Self> {
        let library = Arc::new(Library::open(data_dir)?);
        let store = settings::Store::new(data_dir);
        let saved = store.load();
        let output = output.unwrap_or_else(|| backend_for(&saved));
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
        let analyzer = Arc::new(loudness::Analyzer::new(
            Arc::clone(&library),
            Arc::clone(&emit),
        ));
        analyzer.auto.store(
            saved.replay_gain.mode != ReplayGainMode::Off,
            Ordering::Release,
        );
        let scanner = Arc::new(Scanner {
            library: Arc::clone(&library),
            analyzer: Arc::clone(&analyzer),
            status: Mutex::new(ScanStatus::default()),
            running: AtomicBool::new(false),
            again: AtomicBool::new(false),
            emit,
        });
        engine.set_dsp(saved.current_dsp());
        engine.set_replay_gain(saved.replay_gain);
        engine.set_crossfade(saved.crossfade_s);
        let editor = Arc::new(edit::Editor::new(
            Arc::clone(&library),
            Arc::clone(&scanner),
            Arc::clone(&scanner.emit),
            data_dir.to_owned(),
        ));
        let me = Self {
            library,
            engine,
            settings: store,
            themes: themes::Themes::new(data_dir),
            current: Mutex::new(saved),
            analyzer,
            scanner,
            editor,
            _watcher: Mutex::new(None),
        };
        me.restart_watcher();
        // Catch up with changes made while Cadenza was closed (incremental: seconds even for a
        // large library). The watcher only sees changes from now on.
        if !me.library.roots()?.is_empty() {
            me.scanner.request();
        }
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

    /// The column browser: genres, then artists in a genre, then their albums.
    pub fn browse(&self, genre: Option<&str>, artist: Option<&str>) -> Result<Browse> {
        Ok(self.library.browse(genre, artist)?)
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
                album_id: p.album_id,
                gain: TrackGain {
                    track_gain: p.gain.track_gain,
                    track_peak: p.gain.track_peak,
                    album_gain: p.gain.album_gain,
                    album_peak: p.gain.album_peak,
                },
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

    /// An artist's page: releases by type, top songs by plays, albums they appear on.
    pub fn artist_page(&self, name: &str) -> Result<ArtistPage> {
        Ok(self.library.artist_page(name)?)
    }

    /// Home shelves: recently played, recently added, most played.
    pub fn home(&self) -> Result<Home> {
        Ok(self.library.home()?)
    }

    /// The track's lyrics (sidecar `.lrc`, else embedded), if any.
    pub fn lyrics(&self, track_id: i64) -> Result<Option<Lyrics>> {
        Ok(self.library.lyrics(track_id)?)
    }

    /// Dominant colours of an artwork, most prominent first (up to four).
    pub fn artwork_palette(&self, hash: &str) -> Result<Vec<[u8; 3]>> {
        Ok(self.library.artwork_palette(hash)?)
    }

    // ---- outputs --------------------------------------------------------------------------

    /// Lists outputs afresh (devices come and go).
    pub fn outputs(&self) -> Outputs {
        let settings = self.settings();
        Outputs {
            devices: cadenza_engine::output::devices(),
            current: settings.output_device,
            dop: settings.dop_devices,
        }
    }

    /// Switches to output `id` (playback continues there) and remembers the choice.
    pub fn set_output(&self, id: &str) -> Result<()> {
        self.change_output(|s| s.output_device = id.to_owned())
    }

    /// Turns DSD over DoP on or off for device `id`.
    pub fn set_dop(&self, id: &str, on: bool) -> Result<()> {
        self.change_output(|s| {
            s.dop_devices.retain(|d| d != id);
            if on {
                s.dop_devices.push(id.to_owned());
            }
        })
    }

    fn change_output(&self, change: impl FnOnce(&mut Settings)) -> Result<()> {
        let mut settings = self.settings();
        change(&mut settings);
        self.settings.save(&settings)?;
        self.engine.set_output(backend_for(&settings));
        self.engine.set_dsp(settings.current_dsp());
        if let Ok(mut s) = self.current.lock() {
            *s = settings;
        }
        Ok(())
    }

    // ---- DSP ------------------------------------------------------------------------------

    /// The DSP profile of the current output device.
    pub fn dsp(&self) -> DspSettings {
        self.settings().current_dsp()
    }

    /// Replaces the current output device's DSP profile; heard at once.
    pub fn set_dsp(&self, dsp: DspSettings) -> Result<()> {
        let mut settings = self.settings();
        settings
            .dsp
            .insert(settings.output_device.clone(), dsp.clone());
        self.settings.save(&settings)?;
        if let Ok(mut s) = self.current.lock() {
            *s = settings;
        }
        self.engine.set_dsp(dsp);
        Ok(())
    }

    /// Reads an AutoEq `ParametricEQ.txt` (the caller applies it with [`Self::set_dsp`]).
    pub fn import_autoeq(&self, path: &Path) -> Result<ImportedEq> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| Error::Other(format!("{}: {e}", path.display())))?;
        cadenza_engine::dsp::parse_autoeq(&text).map_err(Error::Other)
    }

    /// The EQ's response in dB at `freqs`, for drawing it.
    pub fn eq_curve(&self, preamp_db: f64, bands: &[EqBand], freqs: &[f64]) -> Vec<f64> {
        cadenza_engine::dsp::eq_curve(preamp_db, bands, freqs)
    }

    // ---- ReplayGain ---------------------------------------------------------------------

    pub fn replay_gain(&self) -> ReplayGainSettings {
        self.settings().replay_gain
    }

    /// Applies at once (the current track restarts in place). Turning ReplayGain on also starts
    /// measuring tracks that have no ReplayGain tags.
    pub fn set_replay_gain(&self, rg: ReplayGainSettings) -> Result<()> {
        let mut settings = self.settings();
        settings.replay_gain = rg;
        self.settings.save(&settings)?;
        if let Ok(mut s) = self.current.lock() {
            *s = settings;
        }
        self.engine.set_replay_gain(rg);
        let on = rg.mode != ReplayGainMode::Off;
        self.analyzer.auto.store(on, Ordering::Release);
        if on {
            self.analyzer.start();
        }
        Ok(())
    }

    /// Crossfade length in seconds (0: off).
    pub fn set_crossfade(&self, seconds: f64) -> Result<()> {
        let seconds = seconds.clamp(0.0, 12.0);
        let mut settings = self.settings();
        settings.crossfade_s = seconds;
        self.settings.save(&settings)?;
        if let Ok(mut s) = self.current.lock() {
            *s = settings;
        }
        self.engine.set_crossfade(seconds);
        Ok(())
    }

    pub fn set_appearance(&self, appearance: Appearance) -> Result<()> {
        if appearance.look.to_string().len() > 256 * 1024 {
            return Err(Error::Other("the look settings are too large".into()));
        }
        let mut settings = self.settings();
        settings.appearance = appearance;
        self.settings.save(&settings)?;
        if let Ok(mut s) = self.current.lock() {
            *s = settings;
        }
        Ok(())
    }

    /// The owner's saved themes (built-in ones live in the UI).
    pub fn themes(&self) -> Vec<SavedTheme> {
        self.themes.list()
    }

    /// Saves a theme from the editor: replacing `id`, or as a new theme.
    pub fn save_theme(&self, id: Option<&str>, theme: &serde_json::Value) -> Result<SavedTheme> {
        self.themes.save(id, theme)
    }

    /// Adds a theme from a theme file's text.
    pub fn import_theme(&self, text: &str) -> Result<SavedTheme> {
        self.themes.import(text)
    }

    pub fn delete_theme(&self, id: &str) -> Result<()> {
        self.themes.delete(id)
    }

    /// Writes a theme to `path` as a file to share.
    pub fn export_theme(&self, theme: &serde_json::Value, path: &Path) -> Result<()> {
        themes::export(theme, path)
    }

    /// Tracks' tags as their files hold them now, for the tag editor.
    pub fn track_tags(&self, ids: &[i64]) -> Result<Vec<TrackTags>> {
        self.editor.tags(ids)
    }

    /// Writes tag changes to files in the background (see [`ApiEvent::EditProgress`] and
    /// [`ApiEvent::EditFinished`]); returns the batch id, for undo.
    pub fn edit_tracks(&self, edits: Vec<TrackEdit>, description: &str) -> Result<i64> {
        self.editor
            .start(edits, description, self.settings().tag_backups)
    }

    pub fn edit_status(&self) -> EditStatus {
        self.editor.status()
    }

    /// Stops after the file being written; files already written stay changed.
    pub fn cancel_edit(&self) {
        self.editor.cancel();
    }

    pub fn edit_history(&self, limit: i64) -> Result<Vec<EditBatch>> {
        Ok(self.library.edit_batches(limit)?)
    }

    pub fn edit_changes(&self, batch: i64) -> Result<Vec<EditChange>> {
        Ok(self.library.edit_changes(batch)?)
    }

    /// Writes back the values a batch replaced, where the files still hold what it wrote.
    pub fn undo_edit(&self, batch: i64) -> Result<i64> {
        self.editor.undo(batch, self.settings().tag_backups)
    }

    /// Reads an image file into the artwork cache, for a picture edit ([`PictureEdit`]).
    pub fn import_artwork(&self, path: &Path) -> Result<cadenza_library::StoredArt> {
        self.editor.import(path)
    }

    /// Saves a track's front cover as cover.jpg (or .png) in its folder.
    pub fn save_cover_to_folder(&self, track_id: i64) -> Result<PathBuf> {
        self.editor.save_cover(track_id)
    }

    /// Where the organiser would move each track's file (`pattern`: see the organiser docs).
    pub fn preview_organise(&self, ids: &[i64], pattern: &str) -> Result<Vec<RenamePlan>> {
        self.editor.preview_organise(ids, pattern)
    }

    /// Moves the tracks' files to the paths `pattern` gives, in the background; undoable.
    pub fn organise(&self, ids: &[i64], pattern: &str) -> Result<i64> {
        self.editor.organise(ids, pattern)
    }

    /// Keep a copy of each file in the data folder before its tags are changed.
    pub fn set_tag_backups(&self, on: bool) -> Result<()> {
        let mut settings = self.settings();
        settings.tag_backups = on;
        self.settings.save(&settings)?;
        if let Ok(mut s) = self.current.lock() {
            *s = settings;
        }
        Ok(())
    }

    pub fn loudness_status(&self) -> LoudnessStatus {
        self.analyzer.status()
    }

    /// Measures every track without ReplayGain, in the background.
    pub fn analyze_loudness(&self) {
        self.analyzer.start();
    }

    pub fn settings(&self) -> Settings {
        self.current.lock().map(|s| s.clone()).unwrap_or_default()
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
