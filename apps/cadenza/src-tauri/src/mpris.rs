//! Desktop media controls over MPRIS: media keys, the desktop's media widget and lock-screen
//! controls see what plays and can control it.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use cadenza_api::{Cadenza, PlayerEvent, PlayerState, Status};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tauri::{AppHandle, Manager};

const SEEK_STEP: Duration = Duration::from_secs(10);

/// Runs until the channel closes. Player events come in through `events`.
pub fn run(app: Arc<Cadenza>, handle: AppHandle, events: Receiver<PlayerEvent>) {
    let config = PlatformConfig {
        display_name: "Cadenza",
        dbus_name: "cadenza",
        hwnd: None,
    };
    let Ok(mut controls) = MediaControls::new(config) else {
        // No session bus (no desktop session): nothing to integrate with.
        return;
    };
    let player = Arc::clone(&app);
    let attached = controls.attach(move |event| match event {
        MediaControlEvent::Play => player.play(),
        MediaControlEvent::Pause => player.pause(),
        MediaControlEvent::Toggle => player.toggle(),
        MediaControlEvent::Next => player.next(),
        MediaControlEvent::Previous => player.previous(),
        MediaControlEvent::Stop => player.stop(),
        MediaControlEvent::Seek(dir) => seek_by(&player, dir, SEEK_STEP),
        MediaControlEvent::SeekBy(dir, by) => seek_by(&player, dir, by),
        MediaControlEvent::SetPosition(MediaPosition(at)) => player.seek(at.as_millis() as u64),
        MediaControlEvent::SetVolume(v) => player.set_volume(v.clamp(0.0, 1.0) as f32),
        MediaControlEvent::Raise => {
            if let Some(w) = handle.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        MediaControlEvent::Quit => handle.exit(0),
        MediaControlEvent::OpenUri(_) => {}
    });
    if attached.is_err() {
        return;
    }
    let mut shown_track = None;
    let mut last: Option<PlayerState> = None;
    loop {
        match events.recv_timeout(Duration::from_secs(5)) {
            Ok(PlayerEvent::State(state)) => {
                if state.track_id != shown_track {
                    shown_track = state.track_id;
                    set_metadata(&mut controls, &app, state.track_id);
                }
                let _ = controls.set_playback(playback(&state));
                if last.as_ref().is_none_or(|l| l.volume != state.volume) {
                    let _ = controls.set_volume(f64::from(state.volume));
                }
                last = Some(state);
            }
            // Positions are interpolated by MPRIS clients; refresh them after a seek only.
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn seek_by(app: &Cadenza, dir: SeekDirection, by: Duration) {
    let now = app.player().position_ms;
    let by = by.as_millis() as u64;
    app.seek(match dir {
        SeekDirection::Forward => now + by,
        SeekDirection::Backward => now.saturating_sub(by),
    });
}

fn playback(s: &PlayerState) -> MediaPlayback {
    let progress = Some(MediaPosition(Duration::from_millis(s.position_ms)));
    match s.status {
        Status::Playing => MediaPlayback::Playing { progress },
        Status::Paused => MediaPlayback::Paused { progress },
        Status::Stopped => MediaPlayback::Stopped,
    }
}

fn set_metadata(controls: &mut MediaControls, app: &Cadenza, track_id: Option<i64>) {
    let Some(track) = track_id
        .and_then(|id| app.tracks(&[id]).ok())
        .and_then(|rows| rows.into_iter().next())
    else {
        let _ = controls.set_metadata(MediaMetadata::default());
        return;
    };
    let cover = track
        .art_hash
        .as_deref()
        .and_then(|h| app.artwork(h, 512).ok())
        .map(|p| format!("file://{}", p.display()));
    let _ = controls.set_metadata(MediaMetadata {
        title: Some(&track.title),
        artist: Some(&track.artist),
        album: Some(&track.album),
        cover_url: cover.as_deref(),
        duration: Some(Duration::from_millis(track.duration_ms.max(0) as u64)),
    });
}
