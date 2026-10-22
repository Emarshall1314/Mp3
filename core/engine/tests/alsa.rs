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
            exclusive: false,
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

/// Exclusive mode through ALSA's `file` plugin, which records the exact bytes the device would
/// receive: they must be the file's samples, in the format chosen for the source's depth.
#[test]
fn exclusive_mode_writes_the_files_samples_byte_for_byte() {
    use cadenza_decode::{Chunk, SampleFormat};

    let dir = tempfile::tempdir().unwrap();
    for (id, width) in [
        ("formats/flac_s16_44100", 2usize),
        ("formats/flac_s24_96000", 3),
        ("formats/wav_s32_384000", 4),
    ] {
        let e = manifest().entries.iter().find(|e| e.id == id).unwrap();
        let raw = dir.path().join(format!("{width}.raw"));
        let engine = Engine::new(
            Box::new(Alsa {
                device: format!("file:'{}',raw", raw.display()),
                exclusive: true,
            }),
            |_| {},
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
        let mut info = None;
        while engine.state().status != Status::Stopped
            || start.elapsed() < Duration::from_millis(200)
        {
            info = info.or(engine.state().stream);
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "{id}: still playing"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let info = info.expect("stream info");
        assert!(info.bit_perfect, "{id}: {info:?}");
        drop(engine);

        let mut dec = cadenza_decode::open(&e.file()).unwrap();
        let SampleFormat::Int(_) = dec.spec().format else {
            panic!()
        };
        let mut want = Vec::new();
        while let Some(Chunk::Int(s)) = dec.next_chunk().unwrap() {
            for v in s {
                want.extend_from_slice(&v.to_le_bytes()[..width]);
            }
        }
        let got = std::fs::read(&raw).unwrap();
        assert!(
            got.len() >= want.len(),
            "{id}: {} of {} bytes",
            got.len(),
            want.len()
        );
        assert!(got[..want.len()] == want[..], "{id}: bytes differ");
        // ALSA pads the last period with silence.
        assert!(
            got[want.len()..].iter().all(|&b| b == 0),
            "{id}: trailing data"
        );
    }
}
