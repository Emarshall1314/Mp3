//! Second-order IIR filters from the RBJ Audio EQ Cookbook, in f64, transposed direct form II.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterKind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

/// One EQ band. Gain is ignored by the pass filters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqBand {
    pub kind: FilterKind,
    pub freq: f64,
    pub gain_db: f64,
    pub q: f64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Coefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coefficients {
    /// `None` when the band cannot be realised at this rate (at or above Nyquist, bad Q).
    pub(crate) fn new(band: &EqBand, rate: u32) -> Option<Self> {
        let fs = f64::from(rate);
        if !(band.freq > 0.0 && band.freq < fs * 0.49 && band.q > 0.0 && band.gain_db.is_finite()) {
            return None;
        }
        let a = 10f64.powf(band.gain_db / 40.0);
        let w0 = 2.0 * PI * band.freq / fs;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * band.q);
        let sq = 2.0 * a.sqrt() * alpha;
        let (b0, b1, b2, a0, a1, a2) = match band.kind {
            FilterKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            FilterKind::LowShelf => (
                a * ((a + 1.0) - (a - 1.0) * cos + sq),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - sq),
                (a + 1.0) + (a - 1.0) * cos + sq,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - sq,
            ),
            FilterKind::HighShelf => (
                a * ((a + 1.0) + (a - 1.0) * cos + sq),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - sq),
                (a + 1.0) - (a - 1.0) * cos + sq,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - sq,
            ),
            FilterKind::LowPass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
        };
        Some(Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        })
    }

    /// Magnitude response in dB at `freq`.
    pub(crate) fn response_db(&self, freq: f64, rate: u32) -> f64 {
        let w = 2.0 * PI * freq / f64::from(rate);
        // H(z) at z = e^{jw}: evaluate numerator and denominator as complex numbers.
        let z1 = (w.cos(), -w.sin());
        let z2 = ((2.0 * w).cos(), -(2.0 * w).sin());
        let num = (
            self.b0 + self.b1 * z1.0 + self.b2 * z2.0,
            self.b1 * z1.1 + self.b2 * z2.1,
        );
        let den = (
            1.0 + self.a1 * z1.0 + self.a2 * z2.0,
            self.a1 * z1.1 + self.a2 * z2.1,
        );
        10.0 * ((num.0 * num.0 + num.1 * num.1) / (den.0 * den.0 + den.1 * den.1)).log10()
    }
}

/// A biquad with its own state, for one channel.
#[derive(Debug, Clone)]
pub(crate) struct Biquad {
    c: Coefficients,
    z1: f64,
    z2: f64,
}

impl Biquad {
    pub(crate) fn new(c: Coefficients) -> Self {
        Self {
            c,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    pub(crate) fn process(&mut self, x: f64) -> f64 {
        let y = self.c.b0 * x + self.z1;
        self.z1 = self.c.b1 * x - self.c.a1 * y + self.z2;
        self.z2 = self.c.b2 * x - self.c.a2 * y;
        y
    }

    pub(crate) fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(kind: FilterKind, freq: f64, gain_db: f64, q: f64) -> EqBand {
        EqBand {
            kind,
            freq,
            gain_db,
            q,
        }
    }

    #[test]
    fn responses_match_the_design() {
        let rate = 48_000;
        let peak = Coefficients::new(&band(FilterKind::Peak, 1000.0, 6.0, 1.0), rate).unwrap();
        assert!((peak.response_db(1000.0, rate) - 6.0).abs() < 1e-6);
        assert!(peak.response_db(20.0, rate).abs() < 0.05);
        let low = Coefficients::new(&band(FilterKind::LowShelf, 100.0, -4.0, 0.707), rate).unwrap();
        assert!((low.response_db(10.0, rate) + 4.0).abs() < 0.05);
        assert!(low.response_db(10_000.0, rate).abs() < 0.05);
        assert!(
            (low.response_db(100.0, rate) + 2.0).abs() < 0.05,
            "half gain at the corner"
        );
        let high =
            Coefficients::new(&band(FilterKind::HighShelf, 8000.0, 3.0, 0.707), rate).unwrap();
        assert!((high.response_db(20_000.0, rate) - 3.0).abs() < 0.2);
        let lp = Coefficients::new(&band(FilterKind::LowPass, 1000.0, 0.0, 0.707), rate).unwrap();
        assert!((lp.response_db(1000.0, rate) + 3.01).abs() < 0.05);
        assert!(Coefficients::new(&band(FilterKind::Peak, 30_000.0, 3.0, 1.0), rate).is_none());
    }

    #[test]
    fn filtering_a_sine_applies_the_gain() {
        let rate = 44_100;
        let c = Coefficients::new(&band(FilterKind::Peak, 2000.0, -6.0, 2.0), rate).unwrap();
        let mut f = Biquad::new(c);
        let n = 44_100;
        let out: Vec<f64> = (0..n)
            .map(|i| f.process((2.0 * PI * 2000.0 * i as f64 / f64::from(rate)).sin()))
            .collect();
        let peak = out[n / 2..].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!((20.0 * peak.log10() + 6.0).abs() < 0.05, "{peak}");
    }
}
