//! The ALSA backend end to end, on ALSA's `null` device (no sound card needed).

#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cadenza_engine::output::alsa::Alsa;
use cadenza_engine::{Engine, Event, QueueItem, Status};
use cadenza_testkit::manifest;

#[test]
fn plays_through_alsa_null_device() {
    let e = manifest()
        .entries
        .iter()
        .find(|e| e.id == "formats/flac_s24_96000")
        .unwrap();
    let errors = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&errors);
    let engine = Engine::new(
        Box::new(Alsa {
            device: "null".into(),
        }),
        move |ev| {
            if let Event::Error { message, .. } = ev {
                sink.lock().unwrap().push(message);
            }
        },
    );
    engine.set_queue(
        vec![QueueItem {
            track_id: 1,
            path: e.file(),
            start_frame: None,
            end_frame: None,
        }],
        0,
        true,
    );
    let start = Instant::now();
    while engine.state().status != Status::Playing && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let info = engine.state().stream.expect("stream info");
    assert!(info.output.starts_with("ALSA null"), "{}", info.output);
    assert_eq!((info.sample_rate, info.bits), (96_000, Some(24)));
    while engine.state().status != Status::Stopped && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(engine.state().status, Status::Stopped);
    assert!(
        errors.lock().unwrap().is_empty(),
        "{:?}",
        errors.lock().unwrap()
    );
}
