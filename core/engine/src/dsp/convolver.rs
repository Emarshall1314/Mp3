//! FIR convolution with a uniformly partitioned overlap-save FFT, for room correction and
//! headphone impulse responses. Latency is one block.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub(crate) const BLOCK: usize = 1024;

/// One channel's impulse response, transformed.
#[derive(Debug, Clone)]
pub(crate) struct Kernel {
    partitions: Vec<Vec<Complex<f64>>>,
}

pub(crate) struct Convolver {
    fft: Arc<dyn RealToComplex<f64>>,
    ifft: Arc<dyn ComplexToReal<f64>>,
    channels: usize,
    kernels: Vec<Arc<Kernel>>,
    /// Per channel: the previous and the filling input block (2 × BLOCK).
    window: Vec<Vec<f64>>,
    /// Per channel: spectra of recent input windows, newest first (one per partition).
    history: Vec<Vec<Vec<Complex<f64>>>>,
    filled: usize,
    /// Output ready to hand out, interleaved.
    out: std::collections::VecDeque<f64>,
    scratch_t: Vec<f64>,
    scratch_f: Vec<Complex<f64>>,
    acc: Vec<Complex<f64>>,
}

impl std::fmt::Debug for Convolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Convolver")
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl Kernel {
    pub(crate) fn new(ir: &[f64]) -> Self {
        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(2 * BLOCK);
        let partitions = ir
            .chunks(BLOCK)
            .map(|part| {
                let mut t = vec![0.0; 2 * BLOCK];
                t[..part.len()].copy_from_slice(part);
                let mut f = fft.make_output_vec();
                let _ = fft.process(&mut t, &mut f);
                f
            })
            .collect();
        Self { partitions }
    }
}

impl Convolver {
    /// `kernels`: one per channel.
    pub(crate) fn new(kernels: Vec<Arc<Kernel>>) -> Self {
        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(2 * BLOCK);
        let ifft = planner.plan_fft_inverse(2 * BLOCK);
        let channels = kernels.len();
        let parts = kernels
            .iter()
            .map(|k| k.partitions.len())
            .max()
            .unwrap_or(1);
        let bins = BLOCK + 1;
        Self {
            channels,
            window: vec![vec![0.0; 2 * BLOCK]; channels],
            history: vec![vec![vec![Complex::default(); bins]; parts]; channels],
            filled: 0,
            out: std::iter::repeat_n(0.0, BLOCK * channels).collect(),
            scratch_t: vec![0.0; 2 * BLOCK],
            scratch_f: fft.make_output_vec(),
            acc: vec![Complex::default(); bins],
            kernels,
            fft,
            ifft,
        }
    }

    pub(crate) fn latency(&self) -> usize {
        BLOCK
    }

    fn run_block(&mut self) {
        let mut block_out = vec![0.0; BLOCK * self.channels];
        for c in 0..self.channels {
            self.scratch_t.copy_from_slice(&self.window[c]);
            let _ = self.fft.process(&mut self.scratch_t, &mut self.scratch_f);
            let hist = &mut self.history[c];
            hist.rotate_right(1);
            hist[0].copy_from_slice(&self.scratch_f);
            self.acc.iter_mut().for_each(|v| *v = Complex::default());
            for (x, h) in hist.iter().zip(&self.kernels[c].partitions) {
                for ((a, xv), hv) in self.acc.iter_mut().zip(x).zip(h) {
                    *a += xv * hv;
                }
            }
            // c2r needs real DC and Nyquist bins; ours are real up to rounding.
            self.acc[0].im = 0.0;
            self.acc[BLOCK].im = 0.0;
            let _ = self.ifft.process(&mut self.acc, &mut self.scratch_t);
            let norm = 1.0 / (2 * BLOCK) as f64;
            for (i, v) in self.scratch_t[BLOCK..].iter().enumerate() {
                block_out[i * self.channels + c] = v * norm;
            }
            // Slide: the current block becomes the previous one.
            self.window[c].copy_within(BLOCK.., 0);
        }
        self.out.extend(block_out);
    }

    /// Replaces `buf` (interleaved) with its convolution, delayed by [`Self::latency`] frames.
    pub(crate) fn process(&mut self, buf: &mut [f64]) {
        let ch = self.channels;
        for frame in buf.chunks_exact_mut(ch) {
            for (c, &s) in frame.iter().enumerate() {
                self.window[c][BLOCK + self.filled] = s;
            }
            self.filled += 1;
            if self.filled == BLOCK {
                self.run_block();
                self.filled = 0;
            }
            for s in frame.iter_mut() {
                *s = self.out.pop_front().unwrap_or(0.0);
            }
        }
    }

    pub(crate) fn reset(&mut self) {
        self.window
            .iter_mut()
            .for_each(|w| w.iter_mut().for_each(|s| *s = 0.0));
        self.history
            .iter_mut()
            .flatten()
            .for_each(|h| h.iter_mut().for_each(|v| *v = Complex::default()));
        self.filled = 0;
        self.out.clear();
        self.out
            .extend(std::iter::repeat_n(0.0, BLOCK * self.channels));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_direct_convolution() {
        // A 2.5-block IR so several partitions are involved.
        let ir: Vec<f64> = (0..BLOCK * 5 / 2)
            .map(|i| ((i * 7919) % 101) as f64 / 101.0 - 0.5)
            .map(|v| v * 0.01)
            .collect();
        let input: Vec<f64> = (0..BLOCK * 6)
            .map(|i| ((i * 104_729) % 97) as f64 / 97.0 - 0.5)
            .collect();
        let k = Arc::new(Kernel::new(&ir));
        let mut conv = Convolver::new(vec![k]);
        let mut buf = input.clone();
        for chunk in buf.chunks_mut(333) {
            conv.process(chunk);
        }
        for n in [0, 1, BLOCK - 1, BLOCK * 3 + 17, input.len() - BLOCK - 1] {
            let want: f64 = (0..=n)
                .filter(|k| n - k < ir.len())
                .map(|k| input[k] * ir[n - k])
                .sum();
            let got = buf[n + BLOCK];
            assert!((got - want).abs() < 1e-9, "n={n}: {got} vs {want}");
        }
    }
}
