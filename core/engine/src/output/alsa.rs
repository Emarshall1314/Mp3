//! ALSA output.
//!
//! - **Shared** (`default`, normally PipeWire or PulseAudio): float samples, and the sound server
//!   mixes and resamples.
//! - **Exclusive** (`hw:`): Cadenza owns the device. Rate switching follows the source, ALSA's own
//!   resampling is disabled, and the integer format is chosen to carry the source's bit depth
//!   unchanged, so an untouched source is bit-perfect.

use std::ffi::CString;
use std::time::{Duration, Instant};

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};

use super::reserve::Reservation;
use super::{DeviceFormat, Output, OutputBackend, OutputDevice, Quantizer, StreamRequest};
use crate::{Error, Result};

/// Every playback device on every sound card, as `hw:CARD=<id>,DEV=<n>` (stable across reboots and
/// hot-plug, unlike card numbers).
pub fn devices() -> Vec<OutputDevice> {
    let mut out = Vec::new();
    for card in alsa::card::Iter::new().flatten() {
        let Ok(ctl) = alsa::Ctl::new(&format!("hw:{}", card.get_index()), false) else {
            continue;
        };
        let Ok(info) = ctl.card_info() else { continue };
        let (Ok(card_id), Ok(card_name)) = (info.get_id(), info.get_name()) else {
            continue;
        };
        let pcms: Vec<(i32, String)> = alsa::ctl::DeviceIter::new(&ctl)
            .filter_map(|dev| {
                let pcm = ctl.pcm_info(dev as u32, 0, Direction::Playback).ok()?;
                Some((dev, pcm.get_name().unwrap_or_default().to_owned()))
            })
            .collect();
        let several = pcms.len() > 1;
        for (dev, pcm_name) in pcms {
            out.push(OutputDevice {
                id: format!("hw:CARD={card_id},DEV={dev}"),
                name: if several && !pcm_name.is_empty() {
                    format!("{card_name} · {pcm_name}")
                } else {
                    card_name.to_owned()
                },
                exclusive: true,
            });
        }
    }
    out
}

/// The card number behind `hw:CARD=<id>,…`, `hw:<n>,…` or `plughw:…`, for the reservation.
fn card_index(device: &str) -> Option<i32> {
    let rest = device.split_once(':')?.1;
    let card = rest
        .split(',')
        .next()?
        .trim_start_matches("CARD=")
        .trim_matches(['\'', '"']);
    if let Ok(n) = card.parse() {
        return Some(n);
    }
    let name = CString::new(card).ok()?;
    alsa::card::Card::from_str(&name)
        .ok()
        .map(|c| c.get_index())
}

#[derive(Debug, Clone, Default)]
pub struct Alsa {
    pub device: String,
    pub exclusive: bool,
    /// Send DSD as DoP when the device has no native DSD format. Only for DACs that decode DoP.
    pub dop: bool,
}

/// Linux's EBUSY.
const EBUSY: i32 = 16;

fn err(e: alsa::Error) -> Error {
    if e.errno() == EBUSY {
        Error::Output(
            "the device is in use by another program (usually the sound server): \
             release it or use shared mode"
                .into(),
        )
    } else {
        Error::Output(format!("alsa: {e}"))
    }
}

/// Formats to try for an exclusive stream, best first: the smallest that carries the source's
/// integers exactly, then wider ones; narrower ones only as a last resort (dithered).
fn exclusive_formats(bits: Option<u8>) -> [Format; 4] {
    match bits {
        Some(b) if b <= 16 => [Format::S16LE, Format::S243LE, Format::S24LE, Format::S32LE],
        Some(b) if b <= 24 => [Format::S243LE, Format::S24LE, Format::S32LE, Format::S16LE],
        // 32-bit integer, float and lossy sources: as many bits as the device takes.
        _ => [Format::S32LE, Format::S243LE, Format::S24LE, Format::S16LE],
    }
}

/// Bytes per word in memory and their order, for the integer and DSD formats Cadenza writes.
fn layout(format: Format) -> Option<(usize, bool)> {
    match format {
        Format::S16LE | Format::DSDU16LE => Some((2, false)),
        Format::S243LE => Some((3, false)),
        Format::S24LE | Format::S32LE | Format::DSDU32LE => Some((4, false)),
        Format::DSDU32BE => Some((4, true)),
        Format::DSDU16BE => Some((2, true)),
        Format::DSDU8 => Some((1, false)),
        _ => None,
    }
}

/// The format as the player sees it; `dop` marks a PCM format carrying DoP.
fn device_format(format: Format, dop: bool) -> DeviceFormat {
    let pcm = |bits| {
        if dop {
            DeviceFormat::Dop { bits }
        } else {
            DeviceFormat::Int { bits }
        }
    };
    match format {
        Format::S16LE => pcm(16),
        Format::S243LE | Format::S24LE => pcm(24),
        Format::S32LE => pcm(32),
        Format::DSDU32BE
        | Format::DSDU32LE
        | Format::DSDU16BE
        | Format::DSDU16LE
        | Format::DSDU8 => {
            let (bytes, big_endian) = layout(format).unwrap_or((1, false));
            DeviceFormat::Dsd {
                bytes: bytes as u8,
                big_endian,
            }
        }
        _ => DeviceFormat::Float,
    }
}

/// Native DSD formats first (a driver offers them only for DACs that take DSD), then DoP if the
/// owner enabled it for this device (a DAC without DoP would play it as loud noise).
fn dsd_format(hwp: &HwParams<'_>, dsd_rate: u32, dop: bool) -> Option<(Format, DeviceFormat)> {
    const NATIVE: [Format; 5] = [
        Format::DSDU32BE,
        Format::DSDU32LE,
        Format::DSDU16BE,
        Format::DSDU16LE,
        Format::DSDU8,
    ];
    const DOP: [Format; 3] = [Format::S243LE, Format::S32LE, Format::S24LE];
    let native = NATIVE.into_iter().map(|f| (f, device_format(f, false)));
    let dop = DOP
        .into_iter()
        .filter(|_| dop)
        .map(|f| (f, device_format(f, true)));
    native.chain(dop).find(|(f, kind)| {
        hwp.test_format(*f).is_ok() && hwp.test_rate(kind.dsd_frame_rate(dsd_rate)).is_ok()
    })
}

pub struct AlsaOutput {
    pcm: PCM,
    /// Held while the device is open; released after the PCM closes (field order).
    _reservation: Option<Reservation>,
    device: String,
    rate: u32,
    channels: u16,
    format: Format,
    kind: DeviceFormat,
    exclusive: bool,
    quantizer: Option<Quantizer>,
    ints: Vec<i32>,
    bytes: Vec<u8>,
    floats: Vec<f32>,
    paused: bool,
    can_pause: bool,
}

impl std::fmt::Debug for AlsaOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlsaOutput")
            .field("device", &self.device)
            .field("rate", &self.rate)
            .field("format", &self.format)
            .finish()
    }
}

// SAFETY: the PCM handle is used from one thread at a time (moved into the output thread).
unsafe impl Send for AlsaOutput {}

impl OutputBackend for Alsa {
    fn open(&self, request: StreamRequest) -> Result<Box<dyn Output>> {
        if request.dsd && !self.exclusive {
            return Err(Error::DsdUnsupported);
        }
        let reservation = match (self.exclusive, card_index(&self.device)) {
            (true, Some(card)) => Reservation::acquire(card)?,
            _ => None,
        };
        // A sound server that agreed to release the card closes it asynchronously.
        let deadline = Instant::now() + Duration::from_secs(2);
        let pcm = loop {
            match PCM::new(&self.device, Direction::Playback, false) {
                Ok(pcm) => break pcm,
                Err(e)
                    if e.errno() == EBUSY && reservation.is_some() && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(err(e)),
            }
        };
        let (rate, channels, format, kind, can_pause) = {
            let hwp = HwParams::any(&pcm).map_err(err)?;
            hwp.set_access(Access::RWInterleaved).map_err(err)?;
            if request.dsd {
                // DSD must arrive exactly as asked: format, rate and channel count.
                hwp.set_rate_resample(false).map_err(err)?;
                let (format, kind) =
                    dsd_format(&hwp, request.rate, self.dop).ok_or(Error::DsdUnsupported)?;
                let rate = kind.dsd_frame_rate(request.rate);
                hwp.set_format(format).map_err(err)?;
                hwp.set_channels(u32::from(request.channels))
                    .map_err(|_| Error::DsdUnsupported)?;
                hwp.set_rate(rate, ValueOr::Nearest)
                    .map_err(|_| Error::DsdUnsupported)?;
                hwp.set_buffer_time_near(200_000, ValueOr::Nearest)
                    .map_err(err)?;
                hwp.set_period_time_near(20_000, ValueOr::Nearest)
                    .map_err(err)?;
                pcm.hw_params(&hwp).map_err(err)?;
                let h = pcm.hw_params_current().map_err(err)?;
                if h.get_rate().map_err(err)? != rate {
                    return Err(Error::DsdUnsupported);
                }
                (rate, request.channels, format, kind, h.can_pause())
            } else {
                let format = if self.exclusive {
                    // Never let ALSA's plug layer resample behind our back.
                    hwp.set_rate_resample(false).map_err(err)?;
                    exclusive_formats(request.bits)
                        .into_iter()
                        .find(|f| hwp.test_format(*f).is_ok())
                        .ok_or_else(|| {
                            Error::Output(format!(
                                "{} takes no 16/24/32-bit integer format",
                                self.device
                            ))
                        })?
                } else {
                    Format::float()
                };
                hwp.set_format(format).map_err(err)?;
                if hwp.set_channels(u32::from(request.channels)).is_err() {
                    hwp.set_channels(2).map_err(err)?;
                }
                // The source's rate if the device has it; otherwise the nearest, and the player
                // resamples.
                if hwp.test_rate(request.rate).is_ok() {
                    hwp.set_rate(request.rate, ValueOr::Nearest).map_err(err)?;
                } else {
                    hwp.set_rate_near(request.rate, ValueOr::Nearest)
                        .map_err(err)?;
                }
                // 200 ms buffer in 20 ms periods: robust against scheduling hiccups, still
                // responsive for pause and seek (which drop the buffer).
                hwp.set_buffer_time_near(200_000, ValueOr::Nearest)
                    .map_err(err)?;
                hwp.set_period_time_near(20_000, ValueOr::Nearest)
                    .map_err(err)?;
                pcm.hw_params(&hwp).map_err(err)?;
                let h = pcm.hw_params_current().map_err(err)?;
                let format = h.get_format().map_err(err)?;
                (
                    h.get_rate().map_err(err)?,
                    h.get_channels().map_err(err)? as u16,
                    format,
                    device_format(format, false),
                    h.can_pause(),
                )
            }
        };
        Ok(Box::new(AlsaOutput {
            pcm,
            _reservation: reservation,
            device: self.device.clone(),
            rate,
            channels,
            format,
            kind,
            exclusive: self.exclusive,
            quantizer: match kind {
                DeviceFormat::Float => None,
                DeviceFormat::Int { bits } | DeviceFormat::Dop { bits } => {
                    Some(Quantizer::new(bits))
                }
                DeviceFormat::Dsd { bytes, .. } => Some(Quantizer::new(bytes * 8)),
            },
            ints: Vec::new(),
            bytes: Vec::new(),
            floats: Vec::new(),
            paused: false,
            can_pause,
        }))
    }
}

impl AlsaOutput {
    fn write_bytes(&mut self, frame_bytes: usize) -> Result<()> {
        let io = self.pcm.io_bytes();
        let mut offset = 0;
        while offset < self.bytes.len() {
            match io.writei(&self.bytes[offset..]) {
                Ok(frames) => offset += frames * frame_bytes,
                // Underrun (EPIPE) or suspend: recover and retry; the gap is a dropout, not a stop.
                Err(e) => self.pcm.try_recover(e, true).map_err(err)?,
            }
        }
        Ok(())
    }

    fn write_floats(&mut self) -> Result<()> {
        let io = self.pcm.io_f32().map_err(err)?;
        let ch = self.channels.max(1) as usize;
        let mut offset = 0;
        while offset < self.floats.len() {
            match io.writei(&self.floats[offset..]) {
                Ok(frames) => offset += frames * ch,
                Err(e) => self.pcm.try_recover(e, true).map_err(err)?,
            }
        }
        Ok(())
    }
}

impl Output for AlsaOutput {
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
        let (Some(q), Some((width, big_endian))) = (self.quantizer.as_mut(), layout(self.format))
        else {
            self.floats.clear();
            self.floats.extend(interleaved.iter().map(|&v| v as f32));
            return self.write_floats();
        };
        self.ints.clear();
        q.convert(interleaved, &mut self.ints);
        self.bytes.clear();
        // Right-aligned values: S24_LE keeps the 24-bit value in the low bytes of its word, and a
        // DSD word's oldest byte is its most significant.
        for &v in &self.ints {
            if big_endian {
                self.bytes.extend_from_slice(&v.to_be_bytes()[4 - width..]);
            } else {
                self.bytes.extend_from_slice(&v.to_le_bytes()[..width]);
            }
        }
        self.write_bytes(width * self.channels.max(1) as usize)
    }

    fn delay_frames(&self) -> u64 {
        self.pcm.delay().map(|d| d.max(0) as u64).unwrap_or(0)
    }

    fn flush(&mut self) -> Result<()> {
        self.pcm.drop().map_err(err)?;
        self.pcm.prepare().map_err(err)?;
        self.paused = false;
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) -> Result<()> {
        if paused == self.paused {
            return Ok(());
        }
        self.paused = paused;
        if self.can_pause {
            self.pcm.pause(paused).map_err(err)?;
        } else if paused {
            // No hardware pause: drop the buffer; playback restarts from the next write.
            self.pcm.drop().map_err(err)?;
            self.pcm.prepare().map_err(err)?;
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<()> {
        self.pcm.drain().map_err(err)
    }

    fn describe(&self) -> String {
        format!(
            "ALSA {} · {} · {} Hz · {} ch · {}",
            self.device,
            if self.exclusive {
                "exclusive"
            } else {
                "shared"
            },
            self.rate,
            self.channels,
            self.format
        )
    }
}
