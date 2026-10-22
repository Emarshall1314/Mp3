//! Audio outputs. An `OutputBackend` opens an `Output` for a requested format; the device may
//! grant a different rate or channel count, and the player adapts (resample / remix).

#[cfg(target_os = "android")]
pub mod aaudio;
#[cfg(target_os = "linux")]
pub mod alsa;

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::Result;

/// What the player asks an output for: the source's format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamRequest {
    pub rate: u32,
    pub channels: u16,
    /// The source's integer bit depth, so an exclusive device can pick a sample format that carries
    /// it unchanged. `None` for float and lossy sources.
    pub bits: Option<u8>,
}

/// The sample format a device takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum DeviceFormat {
    Float,
    /// Signed integers with this many significant bits (16, 24 or 32).
    Int {
        bits: u8,
    },
}

impl DeviceFormat {
    /// True if integer samples of `bits` pass through this format unchanged.
    pub fn carries(self, bits: u8) -> bool {
        match self {
            // f32 has a 24-bit mantissa.
            Self::Float => bits <= 24,
            Self::Int { bits: b } => bits <= b,
        }
    }
}

/// An open output stream. Samples arrive as interleaved f64 in [-1, 1); the output converts them
/// to the device's format (see [`Quantizer`]).
pub trait Output: Send {
    fn rate(&self) -> u32;
    fn channels(&self) -> u16;
    fn format(&self) -> DeviceFormat;
    /// True when the stream reaches the device directly (no OS mixer in between).
    fn exclusive(&self) -> bool;
    /// Blocks until the device has accepted all samples.
    fn write(&mut self, interleaved: &[f64]) -> Result<()>;
    /// Frames written but not yet heard (device buffer).
    fn delay_frames(&self) -> u64;
    /// Drops buffered audio immediately (seek, stop).
    fn flush(&mut self) -> Result<()>;
    fn set_paused(&mut self, paused: bool) -> Result<()>;
    /// Plays out buffered audio, then returns.
    fn drain(&mut self) -> Result<()>;
    /// What the user sees in the signal path, e.g. "ALSA default (PipeWire)".
    fn describe(&self) -> String;
}

pub trait OutputBackend: Send + Sync {
    fn open(&self, request: StreamRequest) -> Result<Box<dyn Output>>;
}

/// Converts f64 samples to `bits`-bit integers for a device.
///
/// A buffer whose samples all lie on the integer grid (untouched integer sources: no volume,
/// resampling or DSP) converts exactly, which is what makes playback bit-perfect. Anything else is
/// TPDF-dithered (±1 LSB triangular noise), so word-length reduction leaves noise, not distortion.
#[derive(Debug, Clone)]
pub struct Quantizer {
    bits: u8,
    scale: f64,
    rng: u64,
}

impl Quantizer {
    pub fn new(bits: u8) -> Self {
        let bits = bits.clamp(8, 32);
        Self {
            bits,
            scale: (1u64 << (bits - 1)) as f64,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn bits(&self) -> u8 {
        self.bits
    }

    fn uniform(&mut self) -> f64 {
        // xorshift64*: plenty for dither noise.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let r = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (r >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }

    /// Appends the integer samples (right-aligned: a 24-bit sample spans ±2^23).
    pub fn convert(&mut self, input: &[f64], out: &mut Vec<i32>) {
        let (min, max) = (-self.scale, self.scale - 1.0);
        let exact = input.iter().all(|&v| {
            let x = v * self.scale;
            x.fract() == 0.0 && (min..=max).contains(&x)
        });
        out.reserve(input.len());
        if exact {
            out.extend(input.iter().map(|&v| (v * self.scale) as i32));
        } else {
            for &v in input {
                let d = self.uniform() + self.uniform();
                out.push((v * self.scale + d).round().clamp(min, max) as i32);
            }
        }
    }
}

/// Records everything written, per opened stream: used by tests and verification tools.
#[derive(Debug, Default, Clone)]
pub struct Capture {
    pub streams: Arc<Mutex<Vec<CapturedStream>>>,
    /// Simulate a device clock at this many times real time (`None`: as fast as possible).
    pub speed: Option<f64>,
    /// Grant this rate instead of the requested one (exercises resampling).
    pub force_rate: Option<u32>,
    /// Behave like an exclusive device taking this format (default: shared, float).
    pub exclusive: Option<DeviceFormat>,
}

#[derive(Debug, Clone)]
pub struct CapturedStream {
    pub rate: u32,
    pub channels: u16,
    pub format: DeviceFormat,
    /// Exactly what the engine handed to the device.
    pub samples: Vec<f64>,
    /// Sample index at each flush (seek/skip): audio after it is the new position's.
    pub flushes: Vec<usize>,
}

impl Capture {
    pub fn new() -> Self {
        Self::default()
    }

    /// Paced like a real device, `speed`× real time.
    pub fn paced(speed: f64) -> Self {
        Self {
            speed: Some(speed),
            ..Self::default()
        }
    }

    pub fn snapshot(&self) -> Vec<CapturedStream> {
        self.streams.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

struct CaptureOutput {
    streams: Arc<Mutex<Vec<CapturedStream>>>,
    index: usize,
    rate: u32,
    channels: u16,
    format: DeviceFormat,
    exclusive: bool,
    speed: Option<f64>,
}

impl OutputBackend for Capture {
    fn open(&self, request: StreamRequest) -> Result<Box<dyn Output>> {
        let rate = self.force_rate.unwrap_or(request.rate);
        let channels = request.channels;
        let format = self.exclusive.unwrap_or(DeviceFormat::Float);
        let mut s = self
            .streams
            .lock()
            .map_err(|_| crate::Error::Output("capture poisoned".into()))?;
        s.push(CapturedStream {
            rate,
            channels,
            format,
            samples: Vec::new(),
            flushes: Vec::new(),
        });
        Ok(Box::new(CaptureOutput {
            streams: Arc::clone(&self.streams),
            index: s.len() - 1,
            rate,
            channels,
            format,
            exclusive: self.exclusive.is_some(),
            speed: self.speed,
        }))
    }
}

impl Output for CaptureOutput {
    fn rate(&self) -> u32 {
        self.rate
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn format(&self) -> DeviceFormat {
        self.format
    }
    fn exclusive(&self) -> bool {
        self.exclusive
    }
    fn write(&mut self, interleaved: &[f64]) -> Result<()> {
        if let Ok(mut s) = self.streams.lock() {
            s[self.index].samples.extend_from_slice(interleaved);
        }
        if let Some(speed) = self.speed {
            let frames = interleaved.len() / self.channels.max(1) as usize;
            std::thread::sleep(std::time::Duration::from_secs_f64(
                frames as f64 / self.rate as f64 / speed,
            ));
        }
        Ok(())
    }
    fn delay_frames(&self) -> u64 {
        0
    }
    fn flush(&mut self) -> Result<()> {
        if let Ok(mut s) = self.streams.lock() {
            let at = s[self.index].samples.len();
            s[self.index].flushes.push(at);
        }
        Ok(())
    }
    fn set_paused(&mut self, _paused: bool) -> Result<()> {
        Ok(())
    }
    fn drain(&mut self) -> Result<()> {
        Ok(())
    }
    fn describe(&self) -> String {
        "capture".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_sources_convert_exactly() {
        let mut q = Quantizer::new(24);
        let input = [
            -1.0,
            0.0,
            0.5,
            8_388_607.0 / 8_388_608.0,
            -1.0 / 8_388_608.0,
        ];
        let mut out = Vec::new();
        q.convert(&input, &mut out);
        assert_eq!(out, [-8_388_608, 0, 4_194_304, 8_388_607, -1]);
        // 16-bit material in a 32-bit container: shifted, still exact.
        let mut q = Quantizer::new(32);
        out.clear();
        q.convert(&[12_345.0 / 32_768.0], &mut out);
        assert_eq!(out, [12_345 << 16]);
    }

    #[test]
    fn processed_audio_is_dithered_and_clipped() {
        let mut q = Quantizer::new(16);
        let mut out = Vec::new();
        // 0.3 LSB: off the grid. Dither makes the long-run average track the true value.
        let input = vec![0.3 / 32_768.0; 100_000];
        q.convert(&input, &mut out);
        let mean = out.iter().map(|&v| v as f64).sum::<f64>() / out.len() as f64;
        assert!((mean - 0.3).abs() < 0.02, "mean {mean}");
        assert!(out.iter().all(|v| (-1..=2).contains(v)));
        out.clear();
        q.convert(&[1.5, -1.5, 0.1], &mut out);
        assert_eq!(&out[..2], [32_767, -32_768]);
    }
}
