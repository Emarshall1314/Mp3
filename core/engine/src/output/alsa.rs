//! ALSA output. M1 uses shared mode through `default` (PipeWire/PulseAudio on desktops, or
//! `plughw` without a sound server); M2 adds exclusive `hw:` devices.

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};

use super::{Output, OutputBackend};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Alsa {
    pub device: String,
}

impl Default for Alsa {
    fn default() -> Self {
        Self {
            device: "default".into(),
        }
    }
}

fn err(e: alsa::Error) -> Error {
    Error::Output(format!("alsa: {e}"))
}

pub struct AlsaOutput {
    pcm: PCM,
    device: String,
    rate: u32,
    channels: u16,
    paused: bool,
    can_pause: bool,
}

impl std::fmt::Debug for AlsaOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlsaOutput")
            .field("device", &self.device)
            .field("rate", &self.rate)
            .finish()
    }
}

// SAFETY: the PCM handle is used from one thread at a time (moved into the output thread).
unsafe impl Send for AlsaOutput {}

impl OutputBackend for Alsa {
    fn open(&self, rate: u32, channels: u16) -> Result<Box<dyn Output>> {
        let pcm = PCM::new(&self.device, Direction::Playback, false).map_err(err)?;
        let (granted_rate, granted_channels, can_pause) = {
            let hwp = HwParams::any(&pcm).map_err(err)?;
            hwp.set_access(Access::RWInterleaved).map_err(err)?;
            hwp.set_format(Format::float()).map_err(err)?;
            if hwp.set_channels(channels as u32).is_err() {
                hwp.set_channels(2).map_err(err)?;
            }
            hwp.set_rate(rate, ValueOr::Nearest).map_err(err)?;
            // 200 ms buffer in 20 ms periods: robust against scheduling hiccups, still
            // responsive for pause and seek (which drop the buffer).
            hwp.set_buffer_time_near(200_000, ValueOr::Nearest)
                .map_err(err)?;
            hwp.set_period_time_near(20_000, ValueOr::Nearest)
                .map_err(err)?;
            pcm.hw_params(&hwp).map_err(err)?;
            let h = pcm.hw_params_current().map_err(err)?;
            (
                h.get_rate().map_err(err)?,
                h.get_channels().map_err(err)? as u16,
                h.can_pause(),
            )
        };
        Ok(Box::new(AlsaOutput {
            pcm,
            device: self.device.clone(),
            rate: granted_rate,
            channels: granted_channels,
            paused: false,
            can_pause,
        }))
    }
}

impl Output for AlsaOutput {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn write(&mut self, interleaved: &[f32]) -> Result<()> {
        let io = self.pcm.io_f32().map_err(err)?;
        let ch = self.channels.max(1) as usize;
        let mut offset = 0;
        while offset < interleaved.len() {
            match io.writei(&interleaved[offset..]) {
                Ok(frames) => offset += frames * ch,
                // Underrun (EPIPE) or suspend: recover and retry; the gap is a dropout, not a stop.
                Err(e) => self.pcm.try_recover(e, true).map_err(err)?,
            }
        }
        Ok(())
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
            "ALSA {} · {} Hz · {} ch · float",
            self.device, self.rate, self.channels
        )
    }
}
