//! Loudness measurement (EBU R128) for tracks without ReplayGain tags, album by album.
//!
//! Gains follow ReplayGain 2: −18 LUFS reference, so gain = −18 − integrated loudness. Album gain
//! uses the album's combined loudness. Peaks are sample peaks. Results go to the library as
//! Cadenza's own measurements; the files are never written.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

use cadenza_decode::{Bounded, Chunk, DsdToPcm, SampleFormat};
use cadenza_library::{Gain, GainTarget, Library, MeasuredGain};
use ebur128::{EbuR128, Mode};
use rayon::prelude::*;
use serde::Serialize;

use crate::{ApiEvent, Emit};

const REFERENCE_LUFS: f64 = -18.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoudnessStatus {
    pub running: bool,
    pub done: u64,
    pub total: u64,
}

pub(crate) struct Analyzer {
    library: Arc<Library>,
    emit: Emit,
    /// Measure automatically after scans (ReplayGain is on).
    pub(crate) auto: AtomicBool,
    running: AtomicBool,
    done: AtomicU64,
    total: AtomicU64,
}

impl Analyzer {
    pub(crate) fn new(library: Arc<Library>, emit: Emit) -> Self {
        Self {
            library,
            emit,
            auto: AtomicBool::new(false),
            running: AtomicBool::new(false),
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
        }
    }

    pub(crate) fn status(&self) -> LoudnessStatus {
        LoudnessStatus {
            running: self.running.load(Ordering::Acquire),
            done: self.done.load(Ordering::Acquire),
            total: self.total.load(Ordering::Acquire),
        }
    }

    fn report(&self) {
        (self.emit)(ApiEvent::Loudness {
            status: self.status(),
        });
    }

    /// Measures every track still without ReplayGain, in the background. No-op while running.
    pub(crate) fn start(self: &Arc<Self>) {
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let me = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("cadenza-loudness".into())
            .spawn(move || {
                let targets = me.library.tracks_missing_gain().unwrap_or_default();
                me.total.store(targets.len() as u64, Ordering::Release);
                me.done.store(0, Ordering::Release);
                me.report();
                // Tracks arrive ordered by album; consecutive ones from one album form a group.
                let mut groups: Vec<Vec<GainTarget>> = Vec::new();
                for t in targets {
                    match groups.last_mut() {
                        Some(g)
                            if t.track.album_id.is_some()
                                && g[0].track.album_id == t.track.album_id =>
                        {
                            g.push(t);
                        }
                        _ => groups.push(vec![t]),
                    }
                }
                groups.par_iter().for_each(|group| {
                    let measured = measure_album(group);
                    let _ = me.library.store_measured_gain(measured);
                    me.done.fetch_add(group.len() as u64, Ordering::AcqRel);
                    me.report();
                });
                me.running.store(false, Ordering::Release);
                me.report();
            });
        if spawned.is_err() {
            self.running.store(false, Ordering::Release);
        }
    }
}

/// Track gains for each target and, when they share an album, the album gain.
fn measure_album(group: &[GainTarget]) -> Vec<MeasuredGain> {
    let measured: Vec<(i64, EbuR128, f64)> = group
        .iter()
        .filter_map(|t| measure(t).map(|(m, peak)| (t.track.id, m, peak)))
        .collect();
    let album = group.first().and_then(|t| t.track.album_id).is_some() && measured.len() > 1;
    let album_gain = album
        .then(|| EbuR128::loudness_global_multiple(measured.iter().map(|(_, m, _)| m)).ok())
        .flatten()
        .filter(|l| l.is_finite())
        .map(|l| REFERENCE_LUFS - l);
    let album_peak = album.then(|| measured.iter().map(|(_, _, p)| *p).fold(0.0, f64::max));
    measured
        .iter()
        .filter_map(|(id, m, peak)| {
            let loudness = m.loudness_global().ok().filter(|l| l.is_finite())?;
            Some(MeasuredGain {
                track_id: *id,
                gain: Gain {
                    track_gain: Some(REFERENCE_LUFS - loudness),
                    track_peak: Some(*peak),
                    album_gain,
                    album_peak: album_gain.and(album_peak),
                },
            })
        })
        .collect()
}

/// Decodes one track through the meter; `None` if it cannot be read.
fn measure(t: &GainTarget) -> Option<(EbuR128, f64)> {
    let mut dec = cadenza_decode::open(&t.track.path).ok()?;
    if t.track.start_frame.is_some() || t.track.end_frame.is_some() {
        dec =
            Box::new(Bounded::new(dec, t.track.start_frame.unwrap_or(0), t.track.end_frame).ok()?);
    }
    if dec.spec().format == SampleFormat::Dsd {
        dec = Box::new(DsdToPcm::new(dec).ok()?);
    }
    let spec = dec.spec().clone();
    let channels = u32::from(spec.channels.max(1));
    let mut meter = EbuR128::new(
        channels,
        spec.sample_rate,
        Mode::I | Mode::SAMPLE_PEAK | Mode::HISTOGRAM,
    )
    .ok()?;
    let mut buf: Vec<f64> = Vec::new();
    while let Ok(Some(chunk)) = dec.next_chunk() {
        buf.clear();
        match (chunk, spec.format) {
            (Chunk::Int(s), SampleFormat::Int(bits)) => {
                let scale = 1.0 / (1u64 << (bits.clamp(1, 32) - 1)) as f64;
                buf.extend(s.iter().map(|&v| f64::from(v) * scale));
            }
            (Chunk::Float(s), _) => buf.extend(s.iter().map(|&v| f64::from(v))),
            _ => return None,
        }
        meter.add_frames_f64(&buf).ok()?;
    }
    let peak = (0..channels)
        .filter_map(|c| meter.sample_peak(c).ok())
        .fold(0.0, f64::max);
    Some((meter, peak))
}
