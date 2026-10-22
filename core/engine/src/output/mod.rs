//! Audio outputs. An `OutputBackend` opens an `Output` for a requested format; the device may
//! grant a different rate or channel count, and the player adapts (resample / remix).

#[cfg(target_os = "android")]
pub mod aaudio;
#[cfg(target_os = "linux")]
pub mod alsa;

use std::sync::{Arc, Mutex};

use crate::Result;

/// An open output stream taking interleaved f32 samples.
pub trait Output: Send {
    fn rate(&self) -> u32;
    fn channels(&self) -> u16;
    /// Blocks until the device has accepted all samples.
    fn write(&mut self, interleaved: &[f32]) -> Result<()>;
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
    fn open(&self, rate: u32, channels: u16) -> Result<Box<dyn Output>>;
}

/// Records everything written, per opened stream: used by tests and verification tools.
#[derive(Debug, Default, Clone)]
pub struct Capture {
    pub streams: Arc<Mutex<Vec<CapturedStream>>>,
    /// Simulate a device clock at this many times real time (`None`: as fast as possible).
    pub speed: Option<f64>,
    /// Grant this rate instead of the requested one (exercises resampling).
    pub force_rate: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct CapturedStream {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
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
    speed: Option<f64>,
}

impl OutputBackend for Capture {
    fn open(&self, rate: u32, channels: u16) -> Result<Box<dyn Output>> {
        let rate = self.force_rate.unwrap_or(rate);
        let mut s = self
            .streams
            .lock()
            .map_err(|_| crate::Error::Output("capture poisoned".into()))?;
        s.push(CapturedStream {
            rate,
            channels,
            ..Default::default()
        });
        Ok(Box::new(CaptureOutput {
            streams: Arc::clone(&self.streams),
            index: s.len() - 1,
            rate,
            channels,
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
    fn write(&mut self, interleaved: &[f32]) -> Result<()> {
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
