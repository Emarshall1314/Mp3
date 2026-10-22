//! Headphone crossfeed after Bauer's stereophonic-to-binaural method, in the form popularised by
//! bs2b: each ear hears the other channel low-passed and slightly attenuated, while its own channel
//! gets a gentle high boost so the overall tone stays flat for mono content.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CrossfeedLevel {
    /// 700 Hz, 4.5 dB: subtle.
    Default,
    /// 700 Hz, 6 dB: Chu Moy's.
    ChuMoy,
    /// 650 Hz, 9.5 dB: Jan Meier's, strongest.
    Meier,
}

impl CrossfeedLevel {
    fn params(self) -> (f64, f64) {
        match self {
            Self::Default => (700.0, 4.5),
            Self::ChuMoy => (700.0, 6.0),
            Self::Meier => (650.0, 9.5),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::ChuMoy => "Chu Moy",
            Self::Meier => "Jan Meier",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Crossfeed {
    a0_lo: f64,
    b1_lo: f64,
    a0_hi: f64,
    a1_hi: f64,
    b1_hi: f64,
    gain: f64,
    /// Per channel (L, R): low-pass state, high-boost state, previous input.
    lo: [f64; 2],
    hi: [f64; 2],
    prev: [f64; 2],
}

impl Crossfeed {
    pub(crate) fn new(level: CrossfeedLevel, rate: u32) -> Self {
        let (fc_lo, feed_db) = level.params();
        let fs = f64::from(rate);
        let gb_lo = feed_db * -5.0 / 6.0 - 3.0;
        let gb_hi = feed_db / 6.0 - 3.0;
        let g_lo = 10f64.powf(gb_lo / 20.0);
        let g_hi = 1.0 - 10f64.powf(gb_hi / 20.0);
        let fc_hi = fc_lo * 2f64.powf((gb_lo - 20.0 * g_hi.log10()) / 12.0);
        let x = (-2.0 * PI * fc_lo / fs).exp();
        let (b1_lo, a0_lo) = (x, g_lo * (1.0 - x));
        let x = (-2.0 * PI * fc_hi / fs).exp();
        let (b1_hi, a0_hi, a1_hi) = (x, 1.0 - g_hi * (1.0 - x), -x);
        Self {
            a0_lo,
            b1_lo,
            a0_hi,
            a1_hi,
            b1_hi,
            // Mono content passes at unity: direct (high-boosted) plus crossed (low-passed) sum to
            // 1 − G_hi + G_lo at DC.
            gain: 1.0 / (1.0 - g_hi + g_lo),
            lo: [0.0; 2],
            hi: [0.0; 2],
            prev: [0.0; 2],
        }
    }

    /// Processes interleaved stereo in place.
    pub(crate) fn process(&mut self, buf: &mut [f64]) {
        for frame in buf.chunks_exact_mut(2) {
            for (c, &x) in frame.iter().enumerate() {
                self.lo[c] = self.a0_lo * x + self.b1_lo * self.lo[c];
                self.hi[c] = self.a0_hi * x + self.a1_hi * self.prev[c] + self.b1_hi * self.hi[c];
                self.prev[c] = x;
            }
            frame[0] = (self.hi[0] + self.lo[1]) * self.gain;
            frame[1] = (self.hi[1] + self.lo[0]) * self.gain;
        }
    }

    pub(crate) fn reset(&mut self) {
        self.lo = [0.0; 2];
        self.hi = [0.0; 2];
        self.prev = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_stays_mono_at_unity_and_hard_panned_bleeds() {
        let mut cf = Crossfeed::new(CrossfeedLevel::ChuMoy, 44_100);
        // A low tone (well inside the crossfeed band) on both channels: unchanged level.
        let tone = |i: usize| (2.0 * PI * 100.0 * i as f64 / 44_100.0).sin() * 0.5;
        let mut buf: Vec<f64> = (0..44_100).flat_map(|i| [tone(i), tone(i)]).collect();
        cf.process(&mut buf);
        let tail = &buf[44_100..];
        let peak = tail.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.02, "{peak}");
        assert!(tail.chunks_exact(2).all(|f| (f[0] - f[1]).abs() < 1e-12));
        // Left only: the right ear now hears some of it.
        cf.reset();
        let mut buf: Vec<f64> = (0..44_100).flat_map(|i| [tone(i), 0.0]).collect();
        cf.process(&mut buf);
        let right = buf[44_100..]
            .iter()
            .skip(1)
            .step_by(2)
            .fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(right > 0.1 && right < 0.5, "{right}");
    }
}
