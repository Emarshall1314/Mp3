//! DSD → PCM conversion for outputs that cannot take DSD (shared mode, non-DoP DACs).
//!
//! Multistage decimation to 176.4 kHz (or 192 kHz for 48 kHz-family DSD):
//! 1. 1-bit → f64 with a 64-tap FIR, decimating by 8. Per-byte lookup tables make each output
//!    eight table reads instead of 64 multiplies.
//! 2. Repeated 64-tap FIR decimate-by-2 stages down to the output rate.
//!
//! Filters are Kaiser-windowed sincs (β = 9, ~90 dB stopband) in f64. DSD's ±1 bits map to ±1.0
//! with no extra gain, so SACD's 0 dB reference (50 % modulation) lands at −6 dBFS and the +3 dB
//! peaks DSD allows can never clip. The pipeline's group delay is compensated and the length is
//! exact, so DSD albums stay gapless.

use crate::{Chunk, Decoder, Error, Result, SampleFormat, StreamSpec};

const TAPS: usize = 64;
const BETA: f64 = 9.0;
/// The idle pattern DSD encoders emit for silence (zero mean).
const DSD_SILENCE: u8 = 0x69;

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    while term > 1e-12 * sum {
        term *= (x / (2.0 * k)).powi(2);
        sum += term;
        k += 1.0;
    }
    sum
}

/// Kaiser-windowed sinc low-pass, `cutoff` in cycles per input sample, unity DC gain.
fn lowpass(cutoff: f64) -> [f64; TAPS] {
    let mut h = [0.0; TAPS];
    let mid = (TAPS - 1) as f64 / 2.0;
    for (k, tap) in h.iter_mut().enumerate() {
        let t = k as f64 - mid;
        let sinc = if t == 0.0 {
            2.0 * cutoff
        } else {
            (2.0 * std::f64::consts::PI * cutoff * t).sin() / (std::f64::consts::PI * t)
        };
        let r = 2.0 * k as f64 / (TAPS - 1) as f64 - 1.0;
        *tap = sinc * bessel_i0(BETA * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(BETA);
    }
    let sum: f64 = h.iter().sum();
    h.iter_mut().for_each(|t| *t /= sum);
    h
}

/// Decimate-by-2 FIR state for one channel.
#[derive(Clone)]
struct Half {
    hist: [f64; TAPS * 2],
    at: usize,
    odd: bool,
}

impl Half {
    fn new() -> Self {
        Self {
            hist: [0.0; TAPS * 2],
            at: 0,
            odd: false,
        }
    }

    /// Pushes one input; returns an output on every second input.
    fn push(&mut self, x: f64, h: &[f64; TAPS]) -> Option<f64> {
        // Mirrored ring buffer: the newest TAPS values are always contiguous.
        self.hist[self.at] = x;
        self.hist[self.at + TAPS] = x;
        self.at = (self.at + 1) % TAPS;
        self.odd = !self.odd;
        if self.odd {
            return None;
        }
        let window = &self.hist[self.at..self.at + TAPS];
        Some(window.iter().zip(h.iter()).map(|(a, b)| a * b).sum())
    }
}

pub struct DsdToPcm {
    inner: Box<dyn Decoder>,
    spec: StreamSpec,
    factor: u64,
    lut: Box<[[f64; 256]; 8]>,
    half_taps: [f64; TAPS],
    /// Per channel: last 8 DSD bytes (oldest first).
    bytes: Vec<[u8; 8]>,
    /// Per channel, per decimate-by-2 stage.
    halves: Vec<Vec<Half>>,
    discard: usize,
    delay: usize,
    produced: u64,
    flushing: bool,
    per_channel: Vec<Vec<f64>>,
    out: Vec<f32>,
}

impl std::fmt::Debug for DsdToPcm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DsdToPcm")
            .field("factor", &self.factor)
            .field("spec", &self.spec)
            .finish()
    }
}

impl DsdToPcm {
    pub fn new(inner: Box<dyn Decoder>) -> Result<Self> {
        let s = inner.spec().clone();
        if s.format != SampleFormat::Dsd {
            return Err(Error::Unsupported("DsdToPcm needs a DSD stream".into()));
        }
        let out_rate = if s.sample_rate.is_multiple_of(44_100) {
            176_400
        } else {
            192_000
        };
        let factor = (s.sample_rate / out_rate) as u64;
        if factor < 16 || !factor.is_power_of_two() || !s.sample_rate.is_multiple_of(out_rate) {
            return Err(Error::Unsupported(format!("DSD rate {}", s.sample_rate)));
        }
        let stages = (factor / 8).trailing_zeros() as usize;

        // Stage 1 (bits → 1/8 rate): pass ~35 kHz, stop by fs/8 − 40 kHz for DSD64.
        let h = lowpass(1.0 / 16.0);
        let mut lut = Box::new([[0.0; 256]; 8]);
        for (g, table) in lut.iter_mut().enumerate() {
            for (byte, v) in table.iter_mut().enumerate() {
                *v = (0..8)
                    .map(|k| h[g * 8 + k] * if byte & (0x80 >> k) != 0 { 1.0 } else { -1.0 })
                    .sum();
            }
        }
        // Group delay in output samples: 31.5 taps at every stage's input rate.
        let mid = (TAPS - 1) as f64 / 2.0;
        let delay =
            mid / factor as f64 + (1..=stages).map(|j| mid / (1u64 << j) as f64).sum::<f64>();
        let delay = delay.round() as usize;

        let ch = s.channels as usize;
        let spec = StreamSpec {
            codec: "dsd",
            lossless: true,
            sample_rate: out_rate,
            channels: s.channels,
            format: SampleFormat::Float,
            total_frames: s.total_frames.map(|f| f / factor),
        };
        Ok(Self {
            inner,
            spec,
            factor,
            lut,
            half_taps: lowpass(0.22),
            bytes: vec![[DSD_SILENCE; 8]; ch],
            halves: vec![vec![Half::new(); stages]; ch],
            discard: delay,
            delay,
            produced: 0,
            flushing: false,
            per_channel: vec![Vec::new(); ch],
            out: Vec::new(),
        })
    }

    /// Runs interleaved DSD bytes through the filters, appending to `per_channel`.
    fn process(&mut self, dsd: &[u8]) {
        let ch = self.spec.channels as usize;
        for frame in dsd.chunks_exact(ch) {
            for (c, &byte) in frame.iter().enumerate() {
                let hist = &mut self.bytes[c];
                hist.copy_within(1.., 0);
                hist[7] = byte;
                let mut y: f64 = (0..8).map(|g| self.lut[g][hist[g] as usize]).sum();
                let mut emitted = true;
                for stage in self.halves[c].iter_mut() {
                    match stage.push(y, &self.half_taps) {
                        Some(v) => y = v,
                        None => {
                            emitted = false;
                            break;
                        }
                    }
                }
                if emitted {
                    self.per_channel[c].push(y);
                }
            }
        }
    }
}

impl Decoder for DsdToPcm {
    fn spec(&self) -> &StreamSpec {
        &self.spec
    }

    fn next_chunk(&mut self) -> Result<Option<Chunk<'_>>> {
        let ch = self.spec.channels as usize;
        loop {
            if let Some(total) = self.spec.total_frames
                && self.produced >= total
            {
                return Ok(None);
            }
            self.per_channel.iter_mut().for_each(Vec::clear);
            if self.flushing {
                // Push silence through so the delayed tail of the track comes out.
                let tail_bytes = (self.delay as u64 * self.factor / 8 + 16) as usize;
                self.process(&vec![DSD_SILENCE; tail_bytes * ch]);
            } else {
                match self.inner.next_chunk()? {
                    Some(Chunk::Dsd(bytes)) => {
                        let bytes = bytes.to_vec();
                        self.process(&bytes);
                    }
                    Some(_) => return Err(Error::Decode("non-DSD chunk in DSD stream".into())),
                    None => {
                        self.flushing = true;
                        continue;
                    }
                }
            }
            let available = self.per_channel[0].len();
            let skip = self.discard.min(available);
            self.discard -= skip;
            let mut n = available - skip;
            if let Some(total) = self.spec.total_frames {
                n = n.min((total - self.produced) as usize);
            }
            if n == 0 {
                if self.flushing {
                    return Ok(None);
                }
                continue;
            }
            self.out.clear();
            for i in skip..skip + n {
                for c in 0..ch {
                    self.out.push(self.per_channel[c][i] as f32);
                }
            }
            self.produced += n as u64;
            return Ok(Some(Chunk::Float(&self.out)));
        }
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        let reached = self.inner.seek(frame * self.factor)? / self.factor;
        self.bytes.iter_mut().for_each(|b| *b = [DSD_SILENCE; 8]);
        self.halves
            .iter_mut()
            .flatten()
            .for_each(|h| *h = Half::new());
        self.discard = self.delay;
        self.produced = reached;
        self.flushing = false;
        Ok(reached)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowpass_has_unity_dc_gain_and_is_symmetric() {
        let h = lowpass(0.22);
        assert!((h.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        for k in 0..TAPS {
            assert!((h[k] - h[TAPS - 1 - k]).abs() < 1e-15);
        }
    }
}
