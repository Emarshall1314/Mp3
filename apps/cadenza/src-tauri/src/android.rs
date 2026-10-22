//! Android: the bridge the playback service (`plugins/android`, `MediaBridge.kt`) uses to read the
//! playback state and send transport commands. It is JNI rather than plugin calls because the
//! service outlives the activity that plugin calls go through; the exports live here, in the
//! library the service calls into.

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use cadenza_api::{Cadenza, PlayerEvent, PlayerState, Status};
use jni::JNIEnv;
use jni::objects::JClass;
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jlong, jstring};

/// A seek shows up as a position this far from where playback should be.
const JUMP_MS: u64 = 1_500;

static APP: OnceLock<Arc<Cadenza>> = OnceLock::new();

/// The latest state as JSON, with a version that increases on every change.
static SESSION: Mutex<(u64, String)> = Mutex::new((0, String::new()));
static CHANGED: Condvar = Condvar::new();

/// What the media session shows for the current track.
#[derive(Default)]
struct Shown {
    track_id: Option<i64>,
    title: String,
    artist: String,
    album: String,
    art: Option<String>,
}

/// Mirrors player events into the session state until the channel closes.
pub fn run(app: Arc<Cadenza>, events: Receiver<PlayerEvent>) {
    let _ = APP.set(Arc::clone(&app));
    let mut track = Shown::default();
    let mut last: Option<(PlayerState, Instant)> = None;
    for event in events {
        match event {
            PlayerEvent::State(state) => {
                publish(&app, &mut track, &state);
                last = Some((state, Instant::now()));
            }
            // Android moves the position on by itself; only a jump (a seek) needs telling.
            PlayerEvent::Position {
                track_id,
                position_ms,
            } => {
                if let Some((state, at)) = last.as_mut()
                    && state.status == Status::Playing
                    && state.track_id == Some(track_id)
                {
                    let expected = state.position_ms + at.elapsed().as_millis() as u64;
                    if expected.abs_diff(position_ms) > JUMP_MS {
                        state.position_ms = position_ms;
                        *at = Instant::now();
                        publish(&app, &mut track, state);
                    }
                }
            }
            _ => {}
        }
    }
}

fn publish(app: &Cadenza, track: &mut Shown, state: &PlayerState) {
    if track.track_id != state.track_id {
        let row = state
            .track_id
            .and_then(|id| app.tracks(&[id]).ok())
            .and_then(|rows| rows.into_iter().next());
        *track = match row {
            Some(t) => Shown {
                track_id: state.track_id,
                art: t
                    .art_hash
                    .as_deref()
                    .and_then(|h| app.artwork(h, 512).ok())
                    .map(|p| p.display().to_string()),
                title: t.title,
                artist: t.artist,
                album: t.album,
            },
            None => Shown {
                track_id: state.track_id,
                ..Shown::default()
            },
        };
    }
    let status = match state.status {
        Status::Playing => "playing",
        Status::Paused => "paused",
        Status::Stopped => "stopped",
    };
    let mut session = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    session.0 += 1;
    session.1 = serde_json::json!({
        "version": session.0,
        "status": status,
        "title": track.title,
        "artist": track.artist,
        "album": track.album,
        "durationMs": state.duration_ms,
        "positionMs": state.position_ms,
        "artPath": track.art,
    })
    .to_string();
    CHANGED.notify_all();
}

// `MediaBridge` codes; keep in step with MediaBridge.kt.
const PLAY: jint = 0;
const PAUSE: jint = 1;
const TOGGLE: jint = 2;
const NEXT: jint = 3;
const PREVIOUS: jint = 4;
const STOP: jint = 5;
const SEEK: jint = 6;

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_cadenza_android_MediaBridge_command(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    code: jint,
    arg: jlong,
) {
    let Some(app) = APP.get() else { return };
    match code {
        PLAY => app.play(),
        PAUSE => app.pause(),
        TOGGLE => app.toggle(),
        NEXT => app.next(),
        PREVIOUS => app.previous(),
        STOP => app.stop(),
        SEEK => app.seek(arg.max(0) as u64),
        _ => {}
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_cadenza_android_MediaBridge_ready(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jboolean {
    if APP.get().is_some() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_cadenza_android_MediaBridge_waitState<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
    after: jlong,
    timeout_ms: jlong,
) -> jstring {
    let newer = |s: &(u64, String)| s.0 as i64 > after;
    let session = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    let (session, _) = CHANGED
        .wait_timeout_while(
            session,
            Duration::from_millis(timeout_ms.max(0) as u64),
            |s| !newer(s),
        )
        .unwrap_or_else(PoisonError::into_inner);
    if !newer(&session) {
        return std::ptr::null_mut();
    }
    let json = session.1.clone();
    drop(session);
    env.new_string(json)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
