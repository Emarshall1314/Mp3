//! The playback engine. Playback lives here, not in the UI: the webview can be suspended or
//! reloaded and the music keeps playing.
//!
//! Threads:
//! - **player**: owns the queue and decoders; decodes ahead (~2 s) into a lock-free ring.
//! - **output**: moves samples from the ring to the device, applies volume, counts frames played.
//!
//! The next track's samples go into the same ring right after the current one's, so gapless
//! playback is sample-continuous. A track needing a different output format drains the ring,
//! then the output is reopened; that is the only non-gapless case.

mod convert;
pub mod dsp;
pub mod output;
mod player;
mod queue;
mod tap;

use std::sync::{Arc, Mutex};
use std::thread;

use crossbeam_channel::{Sender, unbounded};
use serde::{Deserialize, Serialize};

pub use dsp::DspSettings;
pub use output::{
    Capture, CapturedStream, DeviceFormat, Output, OutputBackend, OutputDevice, Quantizer,
    StreamRequest,
};
pub use queue::{QueueItem, Repeat, TrackGain};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("output: {0}")]
    Output(String),
    #[error("decode: {0}")]
    Decode(#[from] cadenza_decode::Error),
    #[error("the engine has stopped")]
    Stopped,
    /// The output cannot take DSD as DSD; the player converts to PCM instead.
    #[error("the output does not take DSD")]
    DsdUnsupported,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Stopped,
    Playing,
    Paused,
}

/// What is actually playing, for badges and the signal path panel.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    pub codec: String,
    pub lossless: bool,
    pub sample_rate: u32,
    /// Integer bit depth; `None` for float and lossy streams.
    pub bits: Option<u8>,
    pub float: bool,
    pub dsd_rate: Option<u32>,
    pub channels: u16,
    /// The output as opened, e.g. "ALSA default · 44100 Hz · 2 ch · float".
    pub output: String,
    pub output_rate: u32,
    pub output_channels: u16,
    pub output_format: DeviceFormat,
    /// The output reaches the device directly, without the OS mixer.
    pub exclusive: bool,
    /// True when the engine resampled to fit the output.
    pub resampled: bool,
    /// The device receives exactly the file's samples.
    pub bit_perfect: bool,
    /// DSP stages running on this stream, in order (empty: none).
    pub dsp: Vec<String>,
    /// ReplayGain applied to this track, in dB (`None`: ReplayGain off or not applicable).
    pub replay_gain_db: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReplayGainMode {
    Off,
    Track,
    Album,
    /// Album gain while an album plays in order (a neighbour in the queue is from the same
    /// album and shuffle is off), track gain otherwise.
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReplayGainSettings {
    pub mode: ReplayGainMode,
    /// Added to every gain.
    pub preamp_db: f64,
    /// Used for tracks without ReplayGain data.
    pub untagged_db: f64,
    /// Lowers the gain where it would push the track's peak past full scale.
    pub prevent_clipping: bool,
}

impl Default for ReplayGainSettings {
    fn default() -> Self {
        Self {
            mode: ReplayGainMode::Off,
            preamp_db: 0.0,
            untagged_db: 0.0,
            prevent_clipping: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub status: Status,
    /// Entries in the queue; fetch them in windows with [`Engine::queue_ids`].
    pub queue_length: usize,
    /// Changes whenever the queue's contents or order change.
    pub queue_version: u64,
    pub index: Option<usize>,
    pub track_id: Option<i64>,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: f32,
    pub repeat: Repeat,
    pub shuffle: bool,
    pub stream: Option<StreamInfo>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            status: Status::Stopped,
            queue_length: 0,
            queue_version: 0,
            index: None,
            track_id: None,
            position_ms: 0,
            duration_ms: 0,
            volume: 1.0,
            repeat: Repeat::Off,
            shuffle: false,
            stream: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    /// Status, queue, current track or settings changed.
    State(PlayerState),
    /// Periodic position update while playing (~4 Hz).
    Position { track_id: i64, position_ms: u64 },
    /// A track stopped being current: listened for `played_ms`, `completed` if it ended naturally
    /// or was mostly heard (for play counts and scrobbling).
    TrackFinished {
        track_id: i64,
        played_ms: u64,
        completed: bool,
    },
    /// A track could not be played and was skipped.
    Error {
        track_id: Option<i64>,
        message: String,
    },
}

pub(crate) enum Cmd {
    SetQueue {
        items: Vec<QueueItem>,
        start: usize,
        play: bool,
    },
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    PlayIndex(usize),
    Seek(u64),
    Volume(f32),
    InsertNext(Vec<QueueItem>),
    Append(Vec<QueueItem>),
    Remove(usize),
    Move(usize, usize),
    ClearUpcoming,
    Repeat(Repeat),
    Shuffle(bool),
    SetOutput(Box<dyn OutputBackend>),
    SetDsp(Box<DspSettings>),
    SetReplayGain(ReplayGainSettings),
    SetCrossfade(f64),
    Quit,
}

pub struct Engine {
    tx: Sender<Cmd>,
    state: Arc<Mutex<PlayerState>>,
    queue_ids: Arc<Mutex<Vec<i64>>>,
    thread: Option<thread::JoinHandle<()>>,
    tap: Arc<tap::Tap>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

impl Engine {
    pub fn new(
        backend: Box<dyn OutputBackend>,
        on_event: impl Fn(Event) + Send + Sync + 'static,
    ) -> Self {
        let (tx, rx) = unbounded();
        let state = Arc::new(Mutex::new(PlayerState::default()));
        let queue_ids = Arc::new(Mutex::new(Vec::new()));
        let shared = (Arc::clone(&state), Arc::clone(&queue_ids));
        let tap = Arc::new(tap::Tap::default());
        let player_tap = Arc::clone(&tap);
        let thread = thread::Builder::new()
            .name("cadenza-player".into())
            .spawn(move || {
                player::run(
                    rx,
                    backend,
                    shared.0,
                    shared.1,
                    Box::new(on_event),
                    player_tap,
                )
            })
            .ok();
        Self {
            tx,
            state,
            queue_ids,
            thread,
            tap,
        }
    }

    /// Starts or stops copying what plays for the visualizer (off costs nothing).
    pub fn set_visualizer(&self, on: bool) {
        self.tap.set_on(on);
    }

    /// The playing audio's levels in `bands` log-spaced bands from 40 Hz to 16 kHz, each 0 to 1;
    /// empty while the visualizer is off, before audio arrives, or for DSD sent as DSD.
    pub fn spectrum(&self, bands: usize) -> Vec<f32> {
        self.tap.spectrum(bands.min(256))
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn state(&self) -> PlayerState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Track ids of queue entries `offset..offset + limit` (play order).
    pub fn queue_ids(&self, offset: usize, limit: usize) -> Vec<i64> {
        self.queue_ids
            .lock()
            .map(|q| q.iter().skip(offset).take(limit).copied().collect())
            .unwrap_or_default()
    }

    pub fn set_queue(&self, items: Vec<QueueItem>, start: usize, play: bool) {
        self.send(Cmd::SetQueue { items, start, play });
    }
    pub fn play(&self) {
        self.send(Cmd::Play);
    }
    pub fn pause(&self) {
        self.send(Cmd::Pause);
    }
    pub fn toggle(&self) {
        self.send(Cmd::Toggle);
    }
    pub fn stop(&self) {
        self.send(Cmd::Stop);
    }
    pub fn next(&self) {
        self.send(Cmd::Next);
    }
    /// Restarts the track if more than 3 s in, else goes to the previous one.
    pub fn previous(&self) {
        self.send(Cmd::Previous);
    }
    pub fn play_index(&self, index: usize) {
        self.send(Cmd::PlayIndex(index));
    }
    pub fn seek(&self, position_ms: u64) {
        self.send(Cmd::Seek(position_ms));
    }
    /// 0.0–1.0 on a perceptual (cubic) curve.
    pub fn set_volume(&self, volume: f32) {
        self.send(Cmd::Volume(volume.clamp(0.0, 1.0)));
    }
    pub fn insert_next(&self, items: Vec<QueueItem>) {
        self.send(Cmd::InsertNext(items));
    }
    pub fn append(&self, items: Vec<QueueItem>) {
        self.send(Cmd::Append(items));
    }
    pub fn remove(&self, index: usize) {
        self.send(Cmd::Remove(index));
    }
    pub fn move_item(&self, from: usize, to: usize) {
        self.send(Cmd::Move(from, to));
    }
    pub fn clear_upcoming(&self) {
        self.send(Cmd::ClearUpcoming);
    }
    pub fn set_repeat(&self, repeat: Repeat) {
        self.send(Cmd::Repeat(repeat));
    }
    pub fn set_shuffle(&self, on: bool) {
        self.send(Cmd::Shuffle(on));
    }
    /// Replaces the DSP settings; heard within milliseconds.
    pub fn set_dsp(&self, settings: DspSettings) {
        self.send(Cmd::SetDsp(Box::new(settings)));
    }
    /// Replaces the ReplayGain settings; the current track restarts in place to apply them.
    pub fn set_replay_gain(&self, settings: ReplayGainSettings) {
        self.send(Cmd::SetReplayGain(settings));
    }
    /// Crossfades between tracks over `seconds` (0: off, the default). Tracks of one album stay
    /// gapless; tracks whose rate or channel count differ change over without a fade.
    pub fn set_crossfade(&self, seconds: f64) {
        self.send(Cmd::SetCrossfade(seconds));
    }
    /// Switches outputs; playback continues on the new one from the same position.
    pub fn set_output(&self, backend: Box<dyn OutputBackend>) {
        self.send(Cmd::SetOutput(backend));
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
