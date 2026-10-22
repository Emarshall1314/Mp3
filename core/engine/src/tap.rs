//! A look at what is playing, for the visualizer: the output thread copies a mono mix of what it
//! sends to the device into a small circular buffer, only while someone is watching. Reading and
//! the spectrum happen elsewhere, so the output thread does a few stores per sample and nothing
//! that could block or allocate.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use realfft::RealFftPlanner;

/// Samples kept: more than one analysis window.
const SIZE: usize = 8192;
/// Samples per spectrum (about 46 ms at 44.1 kHz).
const WINDOW: usize = 2048;
/// Lowest band edge, Hz.
const LOW_HZ: f64 = 40.0;
/// Highest band edge, Hz (or the Nyquist frequency, if lower).
const HIGH_HZ: f64 = 16_000.0;
/// Levels below this read as silence.
const FLOOR_DB: f64 = -72.0;

#[derive(Debug)]
pub(crate) struct Tap {
    on: AtomicBool,
    rate: AtomicU32,
    /// Total samples written; the newest is at `(written - 1) % SIZE`.
    written: AtomicUsize,
    samples: Box<[AtomicU32]>,
}

impl Default for Tap {
    fn default() -> Self {
        Self {
            on: AtomicBool::new(false),
            rate: AtomicU32::new(44_100),
            written: AtomicUsize::new(0),
            samples: (0..SIZE).map(|_| AtomicU32::new(0)).collect(),
        }
    }
}

impl Tap {
    pub(crate) fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Release);
    }

    pub(crate) fn is_on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    pub(crate) fn set_rate(&self, rate: u32) {
        self.rate.store(rate.max(1), Ordering::Relaxed);
    }

    /// Adds interleaved frames (output thread). The mono mix of each frame is kept.
    pub(crate) fn push(&self, interleaved: &[f64], channels: usize) {
        let mut at = self.written.load(Ordering::Relaxed);
        for frame in interleaved.chunks_exact(channels.max(1)) {
            let mono = frame.iter().sum::<f64>() / frame.len() as f64;
            self.samples[at % SIZE].store((mono as f32).to_bits(), Ordering::Relaxed);
            at = at.wrapping_add(1);
        }
        self.written.store(at, Ordering::Release);
    }

    /// The newest `WINDOW` samples, oldest first; `None` before there are enough.
    fn window(&self) -> Option<Vec<f32>> {
        let end = self.written.load(Ordering::Acquire);
        if end < WINDOW {
            return None;
        }
        Some(
            (end - WINDOW..end)
                .map(|i| f32::from_bits(self.samples[i % SIZE].load(Ordering::Relaxed)))
                .collect(),
        )
    }

    /// Levels of `bands` frequency bands, spaced evenly on a log scale from 40 Hz to 16 kHz, each
    /// 0 (silent) to 1 (full scale). Empty while off or before any audio.
    pub(crate) fn spectrum(&self, bands: usize) -> Vec<f32> {
        if !self.is_on() || bands == 0 {
            return Vec::new();
        }
        let Some(window) = self.window() else {
            return Vec::new();
        };
        let rate = f64::from(self.rate.load(Ordering::Relaxed));
        // Hann window.
        let mut input: Vec<f64> = window
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (WINDOW - 1) as f64).cos();
                f64::from(s) * w
            })
            .collect();
        let fft = RealFftPlanner::<f64>::new().plan_fft_forward(WINDOW);
        let mut spectrum = fft.make_output_vec();
        if fft.process(&mut input, &mut spectrum).is_err() {
            return Vec::new();
        }
        // A full-scale sine with a Hann window peaks at WINDOW / 4.
        let reference = WINDOW as f64 / 4.0;
        let bin_hz = rate / WINDOW as f64;
        let high = HIGH_HZ.min(rate / 2.0);
        let ratio = (high / LOW_HZ).powf(1.0 / bands as f64);
        (0..bands)
            .map(|b| {
                let lo = LOW_HZ * ratio.powi(b as i32);
                let hi = lo * ratio;
                let first = ((lo / bin_hz).floor() as usize).min(spectrum.len() - 1);
                let last = ((hi / bin_hz).ceil() as usize).clamp(first + 1, spectrum.len());
                let peak = spectrum[first..last]
                    .iter()
                    .map(|c| c.norm())
                    .fold(0.0, f64::max);
                let db = 20.0 * (peak / reference).max(1e-12).log10();
                ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0) as f32
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, rate: f64, frames: usize, channels: usize) -> Vec<f64> {
        (0..frames)
            .flat_map(|i| {
                let s = (std::f64::consts::TAU * hz * i as f64 / rate).sin() * 0.5;
                std::iter::repeat_n(s, channels)
            })
            .collect()
    }

    #[test]
    fn off_or_empty_gives_nothing() {
        let tap = Tap::default();
        assert!(tap.spectrum(32).is_empty());
        tap.set_on(true);
        assert!(tap.spectrum(32).is_empty(), "no audio yet");
    }

    #[test]
    fn a_tone_lights_its_band() {
        let tap = Tap::default();
        tap.set_on(true);
        tap.set_rate(48_000);
        tap.push(&sine(1000.0, 48_000.0, 6000, 2), 2);
        let bands = tap.spectrum(32);
        assert_eq!(bands.len(), 32);
        let loudest = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap();
        // The band holding 1 kHz.
        let ratio = (16_000.0f64 / 40.0).powf(1.0 / 32.0);
        let expected = ((1000.0f64 / 40.0).ln() / ratio.ln()).floor() as usize;
        assert!(loudest.abs_diff(expected) <= 1, "{loudest} vs {expected}");
        assert!(bands[loudest] > 0.8, "a -6 dB tone is near the top");
        assert!(bands[0] < 0.3 && bands[31] < 0.3, "far bands stay low");
    }

    #[test]
    fn wraps_around() {
        let tap = Tap::default();
        tap.set_on(true);
        for _ in 0..5 {
            tap.push(&sine(440.0, 44_100.0, 3000, 1), 1);
        }
        assert_eq!(tap.spectrum(8).len(), 8);
    }
}
