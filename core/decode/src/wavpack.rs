//! WavPack via libwavpack (BSD-3): integer, float, hybrid (+.wvc) and native DSD.

use std::ffi::{CString, c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::{Chunk, Decoder, Error, Result, SampleFormat, StreamSpec};

#[repr(C)]
struct WavpackContext {
    _private: [u8; 0],
}

#[link(name = "wavpack")]
unsafe extern "C" {
    fn WavpackOpenFileInput(
        name: *const c_char,
        error: *mut c_char,
        flags: c_int,
        norm_offset: c_int,
    ) -> *mut WavpackContext;
    fn WavpackCloseFile(wpc: *mut WavpackContext) -> *mut WavpackContext;
    fn WavpackGetMode(wpc: *mut WavpackContext) -> c_int;
    fn WavpackGetQualifyMode(wpc: *mut WavpackContext) -> c_int;
    fn WavpackGetNumChannels(wpc: *mut WavpackContext) -> c_int;
    fn WavpackGetSampleRate(wpc: *mut WavpackContext) -> u32;
    fn WavpackGetNativeSampleRate(wpc: *mut WavpackContext) -> u32;
    fn WavpackGetBitsPerSample(wpc: *mut WavpackContext) -> c_int;
    fn WavpackGetNumSamples64(wpc: *mut WavpackContext) -> i64;
    fn WavpackUnpackSamples(wpc: *mut WavpackContext, buffer: *mut i32, samples: u32) -> u32;
    fn WavpackSeekSample64(wpc: *mut WavpackContext, sample: i64) -> c_int;
}

const OPEN_WVC: c_int = 0x1;
const OPEN_DSD_NATIVE: c_int = 0x100;
const MODE_FLOAT: c_int = 0x8;
const MODE_LOSSLESS: c_int = 0x40;
const QMODE_DSD_AUDIO: c_int = 0x30;
const CHUNK_FRAMES: u32 = 4096;

pub(crate) struct WavPack {
    ctx: *mut WavpackContext,
    spec: StreamSpec,
    raw: Vec<i32>,
    floats: Vec<f32>,
    bytes: Vec<u8>,
}

// The context is only ever used through &mut self, one thread at a time.
unsafe impl Send for WavPack {}

impl WavPack {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let name = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| Error::Unsupported("path contains NUL".into()))?;
        let mut err = [0 as c_char; 81];
        // SAFETY: `name` is a valid C string and `err` has the 80+1 bytes libwavpack requires.
        let ctx = unsafe {
            WavpackOpenFileInput(
                name.as_ptr(),
                err.as_mut_ptr(),
                OPEN_WVC | OPEN_DSD_NATIVE,
                0,
            )
        };
        if ctx.is_null() {
            // SAFETY: libwavpack wrote a NUL-terminated message into `err`.
            let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            return Err(Error::Decode(format!("wavpack: {msg}")));
        }
        // SAFETY: ctx is a valid, open context for all calls below.
        let (mode, qmode, channels, rate, native_rate, bits, samples) = unsafe {
            (
                WavpackGetMode(ctx),
                WavpackGetQualifyMode(ctx),
                WavpackGetNumChannels(ctx),
                WavpackGetSampleRate(ctx),
                WavpackGetNativeSampleRate(ctx),
                WavpackGetBitsPerSample(ctx),
                WavpackGetNumSamples64(ctx),
            )
        };
        let dsd = qmode & QMODE_DSD_AUDIO != 0;
        let format = if dsd {
            SampleFormat::Dsd
        } else if mode & MODE_FLOAT != 0 {
            SampleFormat::Float
        } else {
            SampleFormat::Int(bits as u8)
        };
        let spec = StreamSpec {
            codec: "wavpack",
            lossless: mode & MODE_LOSSLESS != 0,
            sample_rate: if dsd { native_rate.max(rate * 8) } else { rate },
            channels: channels as u16,
            format,
            // For DSD libwavpack counts bytes per channel; a frame here is one bit.
            total_frames: (samples >= 0).then_some(if dsd {
                samples as u64 * 8
            } else {
                samples as u64
            }),
        };
        Ok(Self {
            ctx,
            spec,
            raw: Vec::new(),
            floats: Vec::new(),
            bytes: Vec::new(),
        })
    }
}

impl Drop for WavPack {
    fn drop(&mut self) {
        // SAFETY: ctx came from WavpackOpenFileInput and is closed exactly once.
        unsafe { WavpackCloseFile(self.ctx) };
    }
}

impl Decoder for WavPack {
    fn spec(&self) -> &StreamSpec {
        &self.spec
    }

    fn next_chunk(&mut self) -> Result<Option<Chunk<'_>>> {
        let ch = self.spec.channels.max(1) as usize;
        self.raw.resize(CHUNK_FRAMES as usize * ch, 0);
        // SAFETY: `raw` holds CHUNK_FRAMES * channels i32s, as WavpackUnpackSamples requires.
        let n =
            unsafe { WavpackUnpackSamples(self.ctx, self.raw.as_mut_ptr(), CHUNK_FRAMES) } as usize;
        if n == 0 {
            return Ok(None);
        }
        self.raw.truncate(n * ch);
        Ok(Some(match self.spec.format {
            SampleFormat::Int(_) => Chunk::Int(&self.raw),
            SampleFormat::Float => {
                self.floats.clear();
                self.floats
                    .extend(self.raw.iter().map(|&v| f32::from_bits(v as u32)));
                Chunk::Float(&self.floats)
            }
            SampleFormat::Dsd => {
                // One DSD byte (8 bits, MSB first) per channel per "sample".
                self.bytes.clear();
                self.bytes.extend(self.raw.iter().map(|&v| v as u8));
                Chunk::Dsd(&self.bytes)
            }
        }))
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        let (target, unit) = if self.spec.format == SampleFormat::Dsd {
            (frame / 8, 8)
        } else {
            (frame, 1)
        };
        // SAFETY: ctx is valid.
        if unsafe { WavpackSeekSample64(self.ctx, target as i64) } == 0 {
            return Err(Error::Decode("wavpack seek failed".into()));
        }
        Ok(target * unit)
    }
}
