//! A lookahead peak limiter: the gain falls ahead of any peak that would exceed the ceiling (so it
//! never clips) and recovers smoothly afterwards.

use std::collections::VecDeque;

/// −0.3 dBFS: room for dither and inter-sample overs on most DACs.
const CEILING: f64 = 0.966;
const LOOKAHEAD_S: f64 = 0.0015;
const RELEASE_S: f64 = 0.08;

#[derive(Debug, Clone)]
pub(crate) struct Limiter {
    channels: usize,
    lookahead: usize,
    /// Delayed frames (interleaved).
    delay: VecDeque<f64>,
    /// Required gain per frame over the lookahead window, with a monotonic deque for its minimum.
    required: VecDeque<(usize, f64)>,
    frame: usize,
    gain: f64,
    release: f64,
}

impl Limiter {
    pub(crate) fn new(rate: u32, channels: u16) -> Self {
        let lookahead = (f64::from(rate) * LOOKAHEAD_S).ceil() as usize;
        let channels = usize::from(channels.max(1));
        Self {
            channels,
            lookahead,
            delay: std::iter::repeat_n(0.0, lookahead * channels).collect(),
            required: VecDeque::new(),
            frame: 0,
            gain: 1.0,
            release: 1.0 - (-1.0 / (f64::from(rate) * RELEASE_S)).exp(),
        }
    }

    pub(crate) fn latency(&self) -> usize {
        self.lookahead
    }

    pub(crate) fn process(&mut self, buf: &mut [f64]) {
        let ch = self.channels;
        for frame in buf.chunks_exact_mut(ch) {
            let peak = frame.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            let need = if peak > CEILING { CEILING / peak } else { 1.0 };
            while self.required.back().is_some_and(|&(_, g)| g >= need) {
                self.required.pop_back();
            }
            self.required.push_back((self.frame, need));
            while self
                .required
                .front()
                .is_some_and(|&(f, _)| f + self.lookahead < self.frame)
            {
                self.required.pop_front();
            }
            self.frame += 1;
            let target = self.required.front().map_or(1.0, |&(_, g)| g);
            // Instant attack (the lookahead already gives it time), exponential release.
            self.gain = if target < self.gain {
                target
            } else {
                self.gain + (target - self.gain) * self.release
            };
            for s in frame.iter_mut() {
                self.delay.push_back(*s);
                *s = self.delay.pop_front().unwrap_or(0.0) * self.gain;
            }
        }
    }

    pub(crate) fn reset(&mut self) {
        self.delay.iter_mut().for_each(|s| *s = 0.0);
        self.required.clear();
        self.gain = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_exceeds_the_ceiling_and_leaves_quiet_audio_alone() {
        let mut l = Limiter::new(48_000, 2);
        let tone = |i: usize, a: f64| (i as f64 * 0.05).sin() * a;
        let mut loud: Vec<f64> = (0..48_000)
            .flat_map(|i| [tone(i, 2.0), tone(i, 1.5)])
            .collect();
        l.process(&mut loud);
        assert!(loud.iter().all(|v| v.abs() <= CEILING + 1e-12));
        let mut l = Limiter::new(48_000, 2);
        let input: Vec<f64> = (0..4_800)
            .flat_map(|i| [tone(i, 0.5), tone(i, 0.5)])
            .collect();
        let mut quiet = input.clone();
        l.process(&mut quiet);
        let lat = l.latency() * 2;
        assert!(
            quiet[lat..] == input[..input.len() - lat],
            "delayed, otherwise untouched"
        );
    }
}
