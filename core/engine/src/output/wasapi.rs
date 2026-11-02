//! Windows output through WASAPI.
//!
//! - **Shared** (`default`): float samples at the Windows mixer's rate and channel count; the player
//!   resamples and remixes to it (better than Windows' own converter), and Windows mixes.
//! - **Exclusive** (`wasapi:<endpoint id>`): Cadenza owns the device. The source's rate and an
//!   integer format that carries its bit depth unchanged are asked for, so an untouched source is
//!   bit-perfect. DSD is not offered (Windows has no native DSD path without ASIO).
//!
//! Both are polled: each write fills whatever room the device buffer has and waits a few
//! milliseconds when it is full, which matches the blocking [`Output::write`].

use std::time::Duration;

use wasapi::{
    AudioClient, AudioRenderClient, DeviceEnumerator, Direction, SampleType, StreamMode,
    WaveFormat, calculate_period_100ns, initialize_mta,
};

use super::{DeviceFormat, Output, OutputBackend, OutputDevice, Quantizer, StreamRequest};
use crate::{Error, Result};

/// Prefix of the ids of exclusive devices; the rest is the Windows endpoint id.
const PREFIX: &str = "wasapi:";
/// How much audio the device buffer holds, in 100 ns units (200 ms, as on Linux).
const BUFFER_HNS: i64 = 2_000_000;

fn err(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Output(format!("wasapi {what}: {e}"))
}

/// Every active playback endpoint, for exclusive use.
pub fn devices() -> Vec<OutputDevice> {
    let _ = initialize_mta();
    let Ok(enumerator) = DeviceEnumerator::new() else {
        return Vec::new();
    };
    let Ok(collection) = enumerator.get_device_collection(&Direction::Render) else {
        return Vec::new();
    };
    (&collection)
        .into_iter()
        .flatten()
        .filter_map(|d| {
            Some(OutputDevice {
                id: format!("{PREFIX}{}", d.get_id().ok()?),
                name: d.get_friendlyname().ok()?,
                exclusive: true,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct Wasapi {
    pub device: String,
}

/// Integer formats to try for an exclusive stream, best first, as (container, valid) bits: the
/// smallest that carries the source exactly, then wider, narrower last (dithered).
fn exclusive_formats(bits: Option<u8>) -> [(usize, usize); 4] {
    match bits {
        Some(b) if b <= 16 => [(16, 16), (24, 24), (32, 24), (32, 32)],
        Some(b) if b <= 24 => [(24, 24), (32, 24), (32, 32), (16, 16)],
        _ => [(32, 32), (24, 24), (32, 24), (16, 16)],
    }
}

/// Rates to fall back on when the device can't take the source's, nearest first.
fn fallback_rates(rate: u32) -> Vec<u32> {
    let mut rates = vec![
        44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000,
    ];
    rates.retain(|&r| r != rate);
    // Same family (44.1 or 48 kHz multiples) first, then by distance.
    rates.sort_by_key(|&r| (r % 44_100 != rate % 44_100, r.abs_diff(rate)));
    rates
}

impl OutputBackend for Wasapi {
    fn open(&self, request: StreamRequest) -> Result<Box<dyn Output>> {
        if request.dsd {
            return Err(Error::DsdUnsupported);
        }
        let _ = initialize_mta();
        let enumerator = DeviceEnumerator::new().map_err(|e| err("devices", e))?;
        let exclusive = self.device.starts_with(PREFIX);
        let device = if exclusive {
            enumerator
                .get_device(&self.device[PREFIX.len()..])
                .map_err(|_| Error::Output(format!("{} is not connected", self.device)))?
        } else {
            enumerator
                .get_default_device(&Direction::Render)
                .map_err(|e| err("default device", e))?
        };
        let name = device.get_friendlyname().unwrap_or_default();
        let mut client = device.get_iaudioclient().map_err(|e| err("client", e))?;

        let (format, kind) = if exclusive {
            let channels = usize::from(request.channels.max(1));
            let rates = std::iter::once(request.rate).chain(fallback_rates(request.rate));
            let mut found = None;
            'search: for rate in rates {
                for &channels in &[channels, 2] {
                    for (store, valid) in exclusive_formats(request.bits) {
                        let want = WaveFormat::new(
                            store,
                            valid,
                            &SampleType::Int,
                            rate as usize,
                            channels,
                            None,
                        );
                        if let Ok(f) = client.is_supported_exclusive_with_quirks(&want) {
                            found = Some((f, valid as u8));
                            break 'search;
                        }
                    }
                }
            }
            let (format, valid) = found.ok_or_else(|| {
                Error::Output(format!(
                    "{name} takes no 16/24/32-bit format in exclusive mode (or another program has it)"
                ))
            })?;
            (format, DeviceFormat::Int { bits: valid })
        } else {
            let mix = client.get_mixformat().map_err(|e| err("mix format", e))?;
            let format = WaveFormat::new(
                32,
                32,
                &SampleType::Float,
                mix.get_samplespersec() as usize,
                usize::from(mix.get_nchannels()),
                None,
            );
            (format, DeviceFormat::Float)
        };

        let rate = format.get_samplespersec();
        let mode = if exclusive {
            let (default_period, _) = client.get_device_period().map_err(|e| err("period", e))?;
            let period = client
                .calculate_aligned_period_near(default_period, Some(128), &format)
                .map_err(|e| err("period", e))?;
            let buffer = client
                .calculate_aligned_period_near(BUFFER_HNS.max(period), Some(128), &format)
                .map_err(|e| err("buffer", e))?;
            StreamMode::PollingExclusive {
                buffer_duration_hns: buffer,
                period_hns: period,
            }
        } else {
            StreamMode::PollingShared {
                autoconvert: false,
                buffer_duration_hns: BUFFER_HNS,
            }
        };
        if let Err(e) = client.initialize_client(&format, &Direction::Render, &mode) {
            if !exclusive {
                return Err(err("open", e));
            }
            // Drivers that want a differently aligned buffer say so; ask once more with the
            // size they suggest (the documented recovery), on a fresh client.
            let frames = client.get_buffer_size().map_err(|_| err("open", &e))?;
            let aligned = calculate_period_100ns(i64::from(frames), i64::from(rate));
            client = device.get_iaudioclient().map_err(|e| err("client", e))?;
            client
                .initialize_client(
                    &format,
                    &Direction::Render,
                    &StreamMode::PollingExclusive {
                        buffer_duration_hns: aligned.max(BUFFER_HNS),
                        period_hns: aligned,
                    },
                )
                .map_err(|e| {
                    Error::Output(format!(
                        "{name} could not be opened exclusively ({e}): another program may be \
                         using it, or exclusive mode is turned off in its Windows sound settings"
                    ))
                })?;
        }
        let render = client
            .get_audiorenderclient()
            .map_err(|e| err("render", e))?;
        let buffer = client.get_buffer_size().map_err(|e| err("buffer", e))?;
        let channels = format.get_nchannels();
        let width = usize::from(format.get_bitspersample() / 8);
        Ok(Box::new(WasapiOutput {
            client,
            render,
            name,
            rate,
            channels,
            width,
            buffer,
            kind,
            exclusive,
            quantizer: match kind {
                DeviceFormat::Int { bits } => Some(Quantizer::new(bits)),
                _ => None,
            },
            ints: Vec::new(),
            bytes: Vec::new(),
            started: false,
            paused: false,
        }))
    }
}

pub struct WasapiOutput {
    client: AudioClient,
    render: AudioRenderClient,
    name: String,
    rate: u32,
    channels: u16,
    /// Bytes per sample in the device buffer.
    width: usize,
    /// The device buffer, in frames.
    buffer: u32,
    kind: DeviceFormat,
    exclusive: bool,
    quantizer: Option<Quantizer>,
    ints: Vec<i32>,
    bytes: Vec<u8>,
    started: bool,
    paused: bool,
}

impl std::fmt::Debug for WasapiOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasapiOutput")
            .field("device", &self.name)
            .field("rate", &self.rate)
            .field("format", &self.kind)
            .finish()
    }
}

// SAFETY: the COM objects live in the multithreaded apartment (`initialize_mta`), where they may
// be called from any thread; the output is used from one thread at a time.
unsafe impl Send for WasapiOutput {}

impl WasapiOutput {
    fn frame_bytes(&self) -> usize {
        self.width * usize::from(self.channels.max(1))
    }

    /// Hands `self.bytes` to the device as room appears, starting the stream once it has audio.
    fn push(&mut self) -> Result<()> {
        let _ = initialize_mta();
        let frame = self.frame_bytes();
        let mut offset = 0;
        while offset < self.bytes.len() {
            let room = self
                .client
                .get_available_space_in_frames()
                .map_err(|e| err("write", e))? as usize;
            let frames = room.min((self.bytes.len() - offset) / frame);
            if frames == 0 {
                if !self.started && !self.paused {
                    self.start()?;
                }
                // A tenth of the buffer: short enough not to starve it, long enough not to spin.
                std::thread::sleep(Duration::from_millis(
                    (u64::from(self.buffer) * 100 / u64::from(self.rate.max(1))).clamp(2, 20),
                ));
                continue;
            }
            let end = offset + frames * frame;
            self.render
                .write_to_device(frames, &self.bytes[offset..end], None)
                .map_err(|e| err("write", e))?;
            offset = end;
        }
        if !self.started && !self.paused {
            self.start()?;
        }
        Ok(())
    }

    fn start(&mut self) -> Result<()> {
        self.client.start_stream().map_err(|e| err("start", e))?;
        self.started = true;
        Ok(())
    }
}

impl Output for WasapiOutput {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn format(&self) -> DeviceFormat {
        self.kind
    }

    fn exclusive(&self) -> bool {
        self.exclusive
    }

    fn write(&mut self, interleaved: &[f64]) -> Result<()> {
        self.bytes.clear();
        if let Some(q) = self.quantizer.as_mut() {
            self.ints.clear();
            q.convert(interleaved, &mut self.ints);
            // Right-aligned values, little-endian, in the container's width; a 24-bit value in a
            // 32-bit container sits in the top bytes (WASAPI's valid bits are the high ones).
            let shift = (self.width * 8).saturating_sub(usize::from(q.bits()));
            for &v in &self.ints {
                let word = (i64::from(v) << shift) as i32;
                self.bytes
                    .extend_from_slice(&word.to_le_bytes()[..self.width.min(4)]);
            }
        } else {
            for &v in interleaved {
                self.bytes.extend_from_slice(&(v as f32).to_le_bytes());
            }
        }
        self.push()
    }

    fn delay_frames(&self) -> u64 {
        self.client
            .get_current_padding()
            .map(u64::from)
            .unwrap_or(0)
    }

    fn flush(&mut self) -> Result<()> {
        let _ = initialize_mta();
        if self.started {
            self.client.stop_stream().map_err(|e| err("stop", e))?;
            self.started = false;
        }
        self.client.reset_stream().map_err(|e| err("reset", e))?;
        self.paused = false;
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) -> Result<()> {
        if paused == self.paused {
            return Ok(());
        }
        let _ = initialize_mta();
        self.paused = paused;
        if paused && self.started {
            self.client.stop_stream().map_err(|e| err("pause", e))?;
            self.started = false;
        } else if !paused && self.delay_frames() > 0 {
            self.start()?;
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<()> {
        if !self.started && self.delay_frames() > 0 {
            self.start()?;
        }
        while self.delay_frames() > 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }

    fn describe(&self) -> String {
        format!(
            "WASAPI {} · {} · {} Hz · {} ch · {}",
            self.name,
            if self.exclusive {
                "exclusive"
            } else {
                "shared"
            },
            self.rate,
            self.channels,
            match self.kind {
                DeviceFormat::Int { bits } if self.width * 8 != usize::from(bits) =>
                    format!("{bits}-bit in {}", self.width * 8),
                DeviceFormat::Int { bits } => format!("{bits}-bit"),
                _ => "float".into(),
            }
        )
    }
}

impl Drop for WasapiOutput {
    fn drop(&mut self) {
        if self.started {
            let _ = self.client.stop_stream();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::DEFAULT_DEVICE;
    use super::*;

    #[test]
    fn fallback_rates_prefer_the_same_family() {
        assert_eq!(fallback_rates(88_200)[..2], [44_100, 176_400]);
        assert_eq!(fallback_rates(96_000)[0], 48_000);
    }

    #[test]
    fn default_is_not_exclusive() {
        assert!(!DEFAULT_DEVICE.starts_with(PREFIX));
    }
}
