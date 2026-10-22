//! Sample conversion between the decoder and the output: to f64, channel remixing, resampling.
//!
//! The pipeline runs in f64, which holds integer PCM of every depth up to 32 bits exactly. An
//! untouched integer source therefore reaches the output on its integer grid, and the output's
//! [`Quantizer`](crate::output::Quantizer) turns it back into the same integers: bit-perfect.

use audioadapter_buffers::direct::InterleavedSlice;
use cadenza_decode::{Chunk, SampleFormat};
use rubato::{Fft, FixedSync, Resampler as _};

use crate::{Error, Result};

pub(crate) fn to_f64(chunk: Chunk<'_>, format: SampleFormat, out: &mut Vec<f64>) {
    out.clear();
    match (chunk, format) {
        (Chunk::Int(s), SampleFormat::Int(bits)) => {
            let scale = 1.0 / (1u64 << (bits.clamp(1, 32) - 1)) as f64;
            out.extend(s.iter().map(|&v| v as f64 * scale));
        }
        (Chunk::Float(s), _) => out.extend(s.iter().map(|&v| f64::from(v))),
        // DSD is converted to PCM before it reaches here.
        _ => {}
    }
}

/// Remaps channels: mono → duplicated, multichannel → stereo with ITU-R BS.775 coefficients
/// (centre and surrounds at −3 dB, LFE dropped), otherwise channel-for-channel.
pub(crate) fn remix(input: &[f64], from: u16, to: u16, out: &mut Vec<f64>) {
    out.clear();
    let (f, t) = (from.max(1) as usize, to.max(1) as usize);
    if f == t {
        out.extend_from_slice(input);
        return;
    }
    const M3DB: f64 = std::f64::consts::FRAC_1_SQRT_2;
    for frame in input.chunks_exact(f) {
        match (f, t) {
            (1, _) => out.extend(std::iter::repeat_n(frame[0], t)),
            (_, 2) if f >= 3 => {
                // WAVE order: FL FR FC LFE BL BR (SL SR)
                let c = frame[2] * M3DB;
                let (mut l, mut r) = (frame[0] + c, frame[1] + c);
                for (i, s) in frame.iter().enumerate().skip(4) {
                    if i % 2 == 0 {
                        l += s * M3DB;
                    } else {
                        r += s * M3DB;
                    }
                }
                // Normalise so a full-scale signal on every channel cannot clip.
                let norm = 1.0 / (1.0 + M3DB + M3DB * ((f - 4) as f64 / 2.0));
                out.push(l * norm);
                out.push(r * norm);
            }
            _ => {
                for c in 0..t {
                    out.push(frame.get(c).copied().unwrap_or(0.0));
                }
            }
        }
    }
}

/// A streaming fixed-ratio resampler (rubato's FFT resampler: synchronous, high quality).
pub(crate) struct Resampler {
    inner: Fft<f64>,
    channels: usize,
    chunk: usize,
    pending: Vec<f64>,
    out_buf: Vec<f64>,
    /// Leading output frames still to drop (the resampler's own delay).
    drop: usize,
    pub(crate) from: u32,
    pub(crate) to: u32,
}

impl Resampler {
    pub(crate) fn new(from: u32, to: u32, channels: u16) -> Result<Self> {
        let chunk = 1024;
        let ch = channels.max(1) as usize;
        let inner = Fft::<f64>::new(from as usize, to as usize, chunk, ch, FixedSync::Input)
            .map_err(|e| Error::Output(format!("resampler: {e}")))?;
        let drop = inner.output_delay();
        let out_max = inner.output_frames_max();
        Ok(Self {
            inner,
            channels: ch,
            chunk,
            pending: Vec::new(),
            out_buf: vec![0.0; out_max * ch],
            drop,
            from,
            to,
        })
    }

    fn run_chunk(&mut self, input: &[f64], out: &mut Vec<f64>) -> Result<()> {
        let ch = self.channels;
        let adapter_in = InterleavedSlice::new(input, ch, self.chunk)
            .map_err(|e| Error::Output(e.to_string()))?;
        let frames_out_max = self.out_buf.len() / ch;
        let mut adapter_out = InterleavedSlice::new_mut(&mut self.out_buf, ch, frames_out_max)
            .map_err(|e| Error::Output(e.to_string()))?;
        let (_, produced) = self
            .inner
            .process_into_buffer(&adapter_in, &mut adapter_out, None)
            .map_err(|e| Error::Output(format!("resampler: {e}")))?;
        let skip = self.drop.min(produced);
        self.drop -= skip;
        out.extend_from_slice(&self.out_buf[skip * ch..produced * ch]);
        Ok(())
    }

    /// Feeds input; appends whatever output is ready.
    pub(crate) fn process(&mut self, input: &[f64], out: &mut Vec<f64>) -> Result<()> {
        self.pending.extend_from_slice(input);
        let step = self.chunk * self.channels;
        let mut consumed = 0;
        while self.pending.len() - consumed >= step {
            let chunk: Vec<f64> = self.pending[consumed..consumed + step].to_vec();
            self.run_chunk(&chunk, out)?;
            consumed += step;
        }
        self.pending.drain(..consumed);
        Ok(())
    }

    /// Pushes the remaining input through (padding with silence) at the end of the stream.
    pub(crate) fn flush(
        &mut self,
        out: &mut Vec<f64>,
        expected_total_out: Option<u64>,
        produced_so_far: u64,
    ) -> Result<()> {
        let step = self.chunk * self.channels;
        let delay = self.inner.output_delay();
        let mut tail = Vec::new();
        let mut padded = std::mem::take(&mut self.pending);
        // Enough silence to clear the filter delay as well as the partial chunk.
        let needed = padded.len() + (delay + self.chunk) * self.channels;
        padded.resize(needed.div_ceil(step) * step, 0.0);
        for chunk in padded.chunks_exact(step) {
            self.run_chunk(chunk, &mut tail)?;
        }
        if let Some(total) = expected_total_out {
            let keep = total.saturating_sub(produced_so_far) as usize * self.channels;
            tail.truncate(keep.min(tail.len()));
        }
        out.extend_from_slice(&tail);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_convert_exactly() {
        let mut out = Vec::new();
        to_f64(
            Chunk::Int(&[-32768, 0, 16384, 32767]),
            SampleFormat::Int(16),
            &mut out,
        );
        assert_eq!(out, [-1.0, 0.0, 0.5, 32767.0 / 32768.0]);
        to_f64(Chunk::Int(&[i32::MAX]), SampleFormat::Int(32), &mut out);
        assert_eq!(out[0] * 2_147_483_648.0, f64::from(i32::MAX));
    }

    #[test]
    fn remix_mono_and_surround() {
        let mut out = Vec::new();
        remix(&[0.5, -0.5], 1, 2, &mut out);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5]);
        // 5.1 full scale on all channels stays within ±1.
        remix(&[1.0; 6], 6, 2, &mut out);
        assert!(out.iter().all(|v| v.abs() <= 1.0 + 1e-6), "{out:?}");
    }

    #[test]
    fn resampling_keeps_length_and_tone() {
        let (from, to) = (44_100u32, 48_000u32);
        let n = 44_100usize;
        let input: Vec<f64> = (0..n)
            .flat_map(|i| {
                let v = (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / from as f64).sin() * 0.5;
                [v, v]
            })
            .collect();
        let mut r = Resampler::new(from, to, 2).unwrap();
        let mut out = Vec::new();
        for c in input.chunks(2 * 777) {
            r.process(c, &mut out).unwrap();
        }
        let expected = n as u64 * to as u64 / from as u64;
        let produced = (out.len() / 2) as u64;
        r.flush(&mut out, Some(expected), produced).unwrap();
        assert_eq!(out.len() / 2, expected as usize);
        // Phase-aligned with the ideal 1 kHz tone (delay compensated), away from the edges.
        let max_err = (2000..46_000)
            .map(|i| {
                let ideal =
                    (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / to as f64).sin() * 0.5;
                (out[i * 2] - ideal).abs()
            })
            .fold(0.0f64, f64::max);
        assert!(max_err < 1e-3, "max error {max_err}");
    }
}
