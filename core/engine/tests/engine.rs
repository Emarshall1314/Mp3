//! Engine behaviour through the capture output: what would reach the device, sample for sample.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cadenza_decode::{Chunk, SampleFormat};
use cadenza_engine::{Capture, DeviceFormat, Engine, Event, QueueItem, Status};
use cadenza_testkit::{Entry, manifest};

fn entry(id: &str) -> &'static Entry {
    manifest()
        .entries
        .iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("no fixture {id}"))
}

fn item(e: &Entry) -> QueueItem {
    QueueItem {
        track_id: e.id.len() as i64 * 1000 + e.path.len() as i64,
        path: e.file(),
        start_frame: None,
        end_frame: None,
    }
}

/// The file decoded straight to f64, as the engine should deliver it.
fn reference(e: &Entry) -> Vec<f64> {
    let mut dec = cadenza_decode::open(&e.file()).unwrap();
    let fmt = dec.spec().format;
    let mut out = Vec::new();
    while let Some(c) = dec.next_chunk().unwrap() {
        match (c, fmt) {
            (Chunk::Int(s), SampleFormat::Int(bits)) => {
                let scale = 1.0 / (1u64 << (bits - 1)) as f64;
                out.extend(s.iter().map(|&v| v as f64 * scale));
            }
            (Chunk::Float(s), _) => out.extend(s.iter().map(|&v| f64::from(v))),
            _ => panic!("unexpected chunk"),
        }
    }
    out
}

struct Harness {
    engine: Engine,
    capture: Capture,
    events: Arc<Mutex<Vec<Event>>>,
}

fn harness(capture: Capture) -> Harness {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let engine = Engine::new(Box::new(capture.clone()), move |e| {
        sink.lock().unwrap().push(e)
    });
    Harness {
        engine,
        capture,
        events,
    }
}

impl Harness {
    fn wait(
        &self,
        what: &str,
        timeout: Duration,
        cond: impl Fn(&cadenza_engine::PlayerState) -> bool,
    ) {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if cond(&self.engine.state()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for {what}: {:?}", self.engine.state());
    }

    fn finished(&self) -> Vec<(i64, bool)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                Event::TrackFinished {
                    track_id,
                    completed,
                    ..
                } => Some((*track_id, *completed)),
                _ => None,
            })
            .collect()
    }

    fn errors(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                Event::Error { message, .. } => Some(message.clone()),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn gapless_album_reaches_the_output_sample_for_sample() {
    let h = harness(Capture::new());
    let parts: Vec<&Entry> = (1..=3)
        .map(|i| entry(&format!("gapless/flac/{i:02}")))
        .collect();
    h.engine
        .set_queue(parts.iter().map(|e| item(e)).collect(), 0, true);
    h.wait("playback to start", Duration::from_secs(5), |s| {
        s.status == Status::Playing
    });
    h.wait("the album to finish", Duration::from_secs(20), |s| {
        s.status == Status::Stopped
    });
    let streams = h.capture.snapshot();
    assert_eq!(
        streams.len(),
        1,
        "one output stream for one format: no reopen between tracks"
    );
    let expected: Vec<f64> = parts.iter().flat_map(|e| reference(e)).collect();
    assert_eq!(
        streams[0].samples.len(),
        expected.len(),
        "no gap, no overlap"
    );
    assert!(streams[0].samples == expected, "samples differ");
    assert_eq!(h.finished().len(), 3);
    assert!(h.finished().iter().all(|(_, done)| *done));
    assert!(h.errors().is_empty(), "{:?}", h.errors());
}

#[test]
fn format_changes_reopen_the_output_and_keep_every_sample() {
    let h = harness(Capture::new());
    let tracks = [
        entry("formats/flac_s16_44100"),
        entry("formats/flac_s24_96000"),
        entry("formats/wav_f32_44100"),
    ];
    h.engine
        .set_queue(tracks.iter().map(|e| item(e)).collect(), 0, true);
    h.wait("start", Duration::from_secs(5), |s| {
        s.status == Status::Playing
    });
    h.wait("end", Duration::from_secs(20), |s| {
        s.status == Status::Stopped
    });
    let streams = h.capture.snapshot();
    let rates: Vec<u32> = streams.iter().map(|s| s.rate).collect();
    assert_eq!(rates, [44_100, 96_000, 44_100]);
    for (s, e) in streams.iter().zip(tracks) {
        assert!(s.samples == reference(e), "{}: samples differ", e.id);
    }
}

#[test]
fn resampling_when_the_device_insists_on_a_rate() {
    let mut cap = Capture::new();
    cap.force_rate = Some(48_000);
    let h = harness(cap);
    let e = entry("formats/flac_s16_44100");
    h.engine.set_queue(vec![item(e)], 0, true);
    h.wait("start", Duration::from_secs(5), |s| {
        s.status == Status::Playing
    });
    let info = {
        let mut info = None;
        let start = Instant::now();
        while info.is_none() && start.elapsed() < Duration::from_secs(5) {
            info = h.engine.state().stream;
        }
        info.unwrap()
    };
    assert!(info.resampled);
    h.wait("end", Duration::from_secs(20), |s| {
        s.status == Status::Stopped
    });
    let s = &h.capture.snapshot()[0];
    assert_eq!(s.rate, 48_000);
    assert_eq!(
        s.samples.len() / 2,
        48_000,
        "1 s at 44.1 kHz → exactly 48 000 frames"
    );
}

#[test]
fn dsd_plays_through_pcm_conversion() {
    let h = harness(Capture::new());
    let e = entry("formats/dsf_dsd64");
    h.engine.set_queue(vec![item(e)], 0, true);
    h.wait("start", Duration::from_secs(5), |s| {
        s.status == Status::Playing
    });
    let info = h.engine.state().stream.unwrap();
    assert_eq!(info.dsd_rate, Some(2_822_400));
    h.wait("end", Duration::from_secs(20), |s| {
        s.status == Status::Stopped
    });
    let s = &h.capture.snapshot()[0];
    assert_eq!((s.rate, s.samples.len() / 2), (176_400, 176_400));
}

#[test]
fn seek_restarts_exactly_at_the_target() {
    let h = harness(Capture::paced(1.0));
    let e = entry("cue/sidecar_utf8/album"); // 6 s, 44.1 kHz
    h.engine.set_queue(vec![item(e)], 0, true);
    h.wait("some playback", Duration::from_secs(5), |s| {
        s.position_ms > 200
    });
    let asked = Instant::now();
    h.engine.seek(3000);
    h.wait("seek", Duration::from_secs(2), |s| {
        s.position_ms >= 3000 && s.position_ms < 3500
    });
    let latency = asked.elapsed();
    h.wait("end", Duration::from_secs(10), |s| {
        s.status == Status::Stopped
    });
    let s = &h.capture.snapshot()[0];
    assert_eq!(s.flushes.len(), 1);
    let full = reference(e);
    let target = 3000 * 44_100 / 1000 * 2;
    // Everything after the flush is exactly the file from 3.000 s on.
    assert!(
        s.samples[s.flushes[0]..] == full[target..],
        "post-seek audio differs from the source at 3.000 s"
    );
    assert!(
        latency < Duration::from_millis(100),
        "seek took {latency:?}"
    );
}

#[test]
fn pause_resume_next_previous() {
    let h = harness(Capture::paced(4.0));
    let tracks: Vec<QueueItem> = [
        "formats/flac_s16_44100",
        "formats/wav_s16_44100",
        "formats/aiff_s16_44100",
    ]
    .iter()
    .enumerate()
    .map(|(i, id)| QueueItem {
        track_id: i as i64 + 1,
        ..item(entry(id))
    })
    .collect();
    h.engine.set_queue(tracks, 0, true);
    h.wait("play", Duration::from_secs(5), |s| {
        s.status == Status::Playing
    });
    h.engine.pause();
    h.wait("pause", Duration::from_secs(2), |s| {
        s.status == Status::Paused
    });
    std::thread::sleep(Duration::from_millis(50));
    let len = h.capture.snapshot()[0].samples.len();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        h.capture.snapshot()[0].samples.len(),
        len,
        "output kept running while paused"
    );
    h.engine.play();
    h.wait("resume", Duration::from_secs(2), |s| {
        s.status == Status::Playing
    });
    h.engine.next();
    h.wait("next", Duration::from_secs(2), |s| s.track_id == Some(2));
    h.engine.previous();
    h.wait("previous", Duration::from_secs(2), |s| {
        s.track_id == Some(1)
    });
    h.engine.play_index(2);
    h.wait("jump", Duration::from_secs(2), |s| s.track_id == Some(3));
    h.wait("end", Duration::from_secs(10), |s| {
        s.status == Status::Stopped
    });
    let finished = h.finished();
    assert_eq!(finished.last().map(|f| f.0), Some(3));
}

#[test]
fn queue_edits_during_playback_take_effect() {
    let h = harness(Capture::paced(8.0));
    let a = QueueItem {
        track_id: 1,
        ..item(entry("formats/flac_s16_44100"))
    };
    let b = QueueItem {
        track_id: 2,
        ..item(entry("formats/wav_s16_44100"))
    };
    let c = QueueItem {
        track_id: 3,
        ..item(entry("formats/aiff_s16_44100"))
    };
    h.engine.set_queue(vec![a, b], 0, true);
    h.wait("play", Duration::from_secs(5), |s| {
        s.status == Status::Playing
    });
    h.engine.insert_next(vec![c]);
    h.wait("queue updated", Duration::from_secs(2), |s| {
        s.queue_length == 3
    });
    assert_eq!(h.engine.queue_ids(0, 10), [1, 3, 2]);
    h.wait("end", Duration::from_secs(10), |s| {
        s.status == Status::Stopped
    });
    let order: Vec<i64> = h.finished().iter().map(|f| f.0).collect();
    assert_eq!(order, [1, 3, 2]);
}

#[test]
fn unplayable_tracks_are_skipped_with_an_error() {
    let h = harness(Capture::new());
    let bad = QueueItem {
        track_id: 1,
        ..item(entry("corrupt/zero_bytes"))
    };
    let good = QueueItem {
        track_id: 2,
        ..item(entry("formats/flac_s16_44100"))
    };
    h.engine.set_queue(vec![bad, good], 0, true);
    h.wait("the good track", Duration::from_secs(5), |s| {
        s.track_id == Some(2)
    });
    h.wait("end", Duration::from_secs(10), |s| {
        s.status == Status::Stopped
    });
    assert_eq!(h.errors().len(), 1);
}

/// The file's integer samples, straight from the decoder.
fn integers(e: &Entry) -> (u8, Vec<i32>) {
    let mut dec = cadenza_decode::open(&e.file()).unwrap();
    let SampleFormat::Int(bits) = dec.spec().format else {
        panic!("{} is not integer PCM", e.id)
    };
    let mut out = Vec::new();
    while let Some(c) = dec.next_chunk().unwrap() {
        let Chunk::Int(s) = c else { panic!() };
        out.extend_from_slice(s);
    }
    (bits, out)
}

fn playback(capture: Capture, tracks: &[&Entry]) -> (Harness, cadenza_engine::StreamInfo) {
    let h = harness(capture);
    h.engine
        .set_queue(tracks.iter().map(|e| item(e)).collect(), 0, true);
    h.wait("start", Duration::from_secs(5), |s| {
        s.status != Status::Stopped || s.queue_length > 0
    });
    h.wait("end", Duration::from_secs(30), |s| {
        s.status == Status::Stopped
    });
    // The first state event that carried stream info: what the first track played as.
    let events = h.events.lock().unwrap().clone();
    let info = events
        .iter()
        .find_map(|e| match e {
            Event::State(s) => s.stream.clone(),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no stream info: {events:?}"));
    (h, info)
}

#[test]
fn exclusive_output_is_bit_perfect_for_every_integer_depth() {
    for (id, device_bits) in [
        ("formats/flac_s16_44100", 16),
        ("formats/flac_s24_96000", 24),
        ("formats/wav_s32_384000", 32),
        ("formats/wavpack_s32_384000", 32),
        ("formats/flac_s16_48000", 32),
    ] {
        let e = entry(id);
        let mut cap = Capture::new();
        cap.exclusive = Some(DeviceFormat::Int { bits: device_bits });
        let (h, info) = playback(cap, &[e]);
        assert!(info.bit_perfect, "{id}: {info:?}");
        let (bits, ints) = integers(e);
        let scale = (1u64 << (device_bits - 1)) as f64;
        let shift = device_bits - bits;
        let got: Vec<i64> = h.capture.snapshot()[0]
            .samples
            .iter()
            .map(|&v| {
                let x = v * scale;
                assert_eq!(x.fract(), 0.0, "{id}: off the integer grid");
                x as i64
            })
            .collect();
        let want: Vec<i64> = ints.iter().map(|&v| i64::from(v) << shift).collect();
        assert!(got == want, "{id}: samples differ");
    }
}

#[test]
fn shared_output_volume_and_lossy_sources_are_not_bit_perfect() {
    let (_, info) = playback(Capture::new(), &[entry("formats/flac_s16_44100")]);
    assert!(!info.bit_perfect && !info.exclusive);
    let mut cap = Capture::new();
    cap.exclusive = Some(DeviceFormat::Int { bits: 32 });
    let (_, info) = playback(cap.clone(), &[entry("formats/mp3-cbr_s16_44100")]);
    assert!(!info.bit_perfect, "lossy");
    let h = harness(cap);
    h.engine.set_volume(0.5);
    h.engine
        .set_queue(vec![item(entry("formats/flac_s16_44100"))], 0, true);
    h.wait("volume applied", Duration::from_secs(5), |s| {
        s.stream.is_some() && s.volume < 1.0
    });
    assert!(!h.engine.state().stream.unwrap().bit_perfect, "volume");
}

#[test]
fn exclusive_output_reopens_only_when_the_depth_would_not_fit() {
    let tracks = [
        entry("formats/flac_s16_44100"),
        entry("formats/aiff_s16_44100"),
        entry("formats/wav_s16_44100"),
    ];
    // A 32-bit device carries 16-bit tracks: one stream, gapless.
    let mut cap = Capture::new();
    cap.exclusive = Some(DeviceFormat::Int { bits: 32 });
    let (h, _) = playback(cap, &tracks);
    assert_eq!(h.capture.snapshot().len(), 1);
    // Shared output never reopens for depth alone.
    let (h, _) = playback(
        Capture::new(),
        &[
            entry("formats/flac_s16_44100"),
            entry("formats/flac_s16_44100"),
        ],
    );
    assert_eq!(h.capture.snapshot().len(), 1);
}

#[test]
fn switching_outputs_continues_from_the_same_position() {
    let first = Capture::paced(4.0);
    let second = Capture::new();
    let h = harness(first);
    let e = entry("gapless/flac/01");
    h.engine.set_queue(vec![item(e)], 0, true);
    h.wait("some playback", Duration::from_secs(5), |s| {
        s.position_ms >= 300
    });
    h.engine.set_output(Box::new(second.clone()));
    h.wait("end", Duration::from_secs(20), |s| {
        s.status == Status::Stopped
    });
    let full = reference(e);
    let a = &h.capture.snapshot()[0].samples;
    let b = &second.snapshot()[0].samples;
    assert!(!a.is_empty() && !b.is_empty());
    // The second output starts where the listener was, and plays to the end.
    let resumed_at = full.len() - b.len();
    assert!(b[..] == full[resumed_at..], "tail differs");
    let heard = resumed_at as f64 / 2.0 / 44_100.0;
    assert!((0.25..2.0).contains(&heard), "resumed at {heard:.2} s");
    assert!(h.errors().is_empty(), "{:?}", h.errors());
}
