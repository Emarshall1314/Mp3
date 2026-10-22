//! Android output through AAudio (API 26+), shared mode. The OS mixer resamples to the device rate;
//! the bit-perfect USB path (API 34 mixer attributes) comes with M2.
//!
//! AAudio is a small C API in `libaaudio.so`, bound by hand here like libwavpack in the decoder.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::time::{Duration, Instant};

use super::{Output, OutputBackend};
use crate::{Error, Result};

#[repr(C)]
struct AAudioStreamBuilder {
    _private: [u8; 0],
}
#[repr(C)]
struct AAudioStream {
    _private: [u8; 0],
}

type AResult = i32;

const AAUDIO_OK: AResult = 0;
const AAUDIO_ERROR_DISCONNECTED: AResult = -899;
const AAUDIO_ERROR_TIMEOUT: AResult = -885;
const AAUDIO_DIRECTION_OUTPUT: i32 = 0;
const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
const AAUDIO_SHARING_MODE_SHARED: i32 = 1;
const AAUDIO_PERFORMANCE_MODE_POWER_SAVING: i32 = 11;
const AAUDIO_STREAM_STATE_PAUSED: i32 = 6;
const AAUDIO_STREAM_STATE_FLUSHED: i32 = 8;

const NANOS_PER_SECOND: i64 = 1_000_000_000;

#[link(name = "aaudio")]
unsafe extern "C" {
    fn AAudio_createStreamBuilder(builder: *mut *mut AAudioStreamBuilder) -> AResult;
    fn AAudio_convertResultToText(result: AResult) -> *const c_char;
    fn AAudioStreamBuilder_setDirection(builder: *mut AAudioStreamBuilder, direction: i32);
    fn AAudioStreamBuilder_setSampleRate(builder: *mut AAudioStreamBuilder, rate: i32);
    fn AAudioStreamBuilder_setChannelCount(builder: *mut AAudioStreamBuilder, channels: i32);
    fn AAudioStreamBuilder_setFormat(builder: *mut AAudioStreamBuilder, format: i32);
    fn AAudioStreamBuilder_setSharingMode(builder: *mut AAudioStreamBuilder, mode: i32);
    fn AAudioStreamBuilder_setPerformanceMode(builder: *mut AAudioStreamBuilder, mode: i32);
    fn AAudioStreamBuilder_openStream(
        builder: *mut AAudioStreamBuilder,
        stream: *mut *mut AAudioStream,
    ) -> AResult;
    fn AAudioStreamBuilder_delete(builder: *mut AAudioStreamBuilder) -> AResult;
    fn AAudioStream_close(stream: *mut AAudioStream) -> AResult;
    fn AAudioStream_requestStart(stream: *mut AAudioStream) -> AResult;
    fn AAudioStream_requestPause(stream: *mut AAudioStream) -> AResult;
    fn AAudioStream_requestFlush(stream: *mut AAudioStream) -> AResult;
    fn AAudioStream_requestStop(stream: *mut AAudioStream) -> AResult;
    fn AAudioStream_waitForStateChange(
        stream: *mut AAudioStream,
        input_state: i32,
        next_state: *mut i32,
        timeout_nanos: i64,
    ) -> AResult;
    fn AAudioStream_getState(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_write(
        stream: *mut AAudioStream,
        buffer: *const c_void,
        num_frames: i32,
        timeout_nanos: i64,
    ) -> AResult;
    fn AAudioStream_getSampleRate(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getChannelCount(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getFormat(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getBufferCapacityInFrames(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_setBufferSizeInFrames(stream: *mut AAudioStream, frames: i32) -> AResult;
    fn AAudioStream_getFramesWritten(stream: *mut AAudioStream) -> i64;
    fn AAudioStream_getFramesRead(stream: *mut AAudioStream) -> i64;
}

fn text(result: AResult) -> String {
    // SAFETY: AAudio returns a static string for every result code, or null.
    let p = unsafe { AAudio_convertResultToText(result) };
    if p.is_null() {
        format!("error {result}")
    } else {
        // SAFETY: non-null, NUL-terminated, static.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

fn check(result: AResult, what: &str) -> Result<()> {
    if result == AAUDIO_OK {
        Ok(())
    } else {
        Err(Error::Output(format!("aaudio {what}: {}", text(result))))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AAudio;

impl OutputBackend for AAudio {
    fn open(&self, rate: u32, channels: u16) -> Result<Box<dyn Output>> {
        let stream = open_stream(rate, channels)?;
        Ok(Box::new(stream))
    }
}

#[derive(Debug)]
pub struct AAudioOutput {
    stream: *mut AAudioStream,
    rate: u32,
    channels: u16,
    paused: bool,
}

// SAFETY: the stream handle is used from one thread at a time (moved into the output thread);
// AAudio allows calls from any thread.
unsafe impl Send for AAudioOutput {}

fn open_stream(rate: u32, channels: u16) -> Result<AAudioOutput> {
    let mut builder = ptr::null_mut();
    // SAFETY: plain C calls on handles AAudio created; the builder is deleted on every path.
    unsafe {
        check(AAudio_createStreamBuilder(&mut builder), "create builder")?;
        AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_OUTPUT);
        AAudioStreamBuilder_setSampleRate(builder, rate as i32);
        AAudioStreamBuilder_setChannelCount(builder, i32::from(channels));
        AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_FLOAT);
        AAudioStreamBuilder_setSharingMode(builder, AAUDIO_SHARING_MODE_SHARED);
        // Music playback: a deep buffer saves battery; latency only affects pause and seek, which
        // flush the buffer anyway.
        AAudioStreamBuilder_setPerformanceMode(builder, AAUDIO_PERFORMANCE_MODE_POWER_SAVING);
        let mut stream = ptr::null_mut();
        let opened = AAudioStreamBuilder_openStream(builder, &mut stream);
        AAudioStreamBuilder_delete(builder);
        check(opened, "open stream")?;
        let out = AAudioOutput {
            stream,
            rate: AAudioStream_getSampleRate(stream).max(1) as u32,
            channels: AAudioStream_getChannelCount(stream).max(1) as u16,
            paused: false,
        };
        // `out` closes the stream on drop from here on.
        if AAudioStream_getFormat(stream) != AAUDIO_FORMAT_PCM_FLOAT {
            return Err(Error::Output("aaudio: float output not granted".into()));
        }
        AAudioStream_setBufferSizeInFrames(stream, AAudioStream_getBufferCapacityInFrames(stream));
        check(AAudioStream_requestStart(stream), "start")?;
        Ok(out)
    }
}

impl AAudioOutput {
    /// Waits until the stream leaves its current state for `target` (or fails / times out).
    fn wait_for(&self, target: i32) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        // SAFETY: valid stream handle.
        let mut state = unsafe { AAudioStream_getState(self.stream) };
        while state != target {
            if Instant::now() > deadline {
                return Err(Error::Output("aaudio: state change timed out".into()));
            }
            let mut next = state;
            // SAFETY: valid stream handle and out pointer.
            let r = unsafe {
                AAudioStream_waitForStateChange(self.stream, state, &mut next, NANOS_PER_SECOND / 5)
            };
            // A timeout just loops back to the deadline check.
            if r != AAUDIO_OK && r != AAUDIO_ERROR_TIMEOUT {
                return check(r, "wait for state");
            }
            state = next;
        }
        Ok(())
    }

    /// The device went away (headphones unplugged, Bluetooth disconnected): open a new stream on
    /// the new default device with the same format, so playback continues there.
    fn reopen(&mut self) -> Result<()> {
        let new = open_stream(self.rate, self.channels)?;
        if new.rate != self.rate || new.channels != self.channels {
            return Err(Error::Output(
                "aaudio: the new output device has a different format".into(),
            ));
        }
        // The old stream is closed when `new` replaces it.
        *self = new;
        Ok(())
    }
}

impl Output for AAudioOutput {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn write(&mut self, interleaved: &[f32]) -> Result<()> {
        let ch = self.channels.max(1) as usize;
        let mut offset = 0;
        while offset < interleaved.len() {
            let frames = ((interleaved.len() - offset) / ch) as i32;
            if frames == 0 {
                break;
            }
            // SAFETY: the pointer covers `frames * ch` valid samples.
            let r = unsafe {
                AAudioStream_write(
                    self.stream,
                    interleaved[offset..].as_ptr().cast(),
                    frames,
                    NANOS_PER_SECOND,
                )
            };
            match r {
                n if n >= 0 => offset += n as usize * ch,
                AAUDIO_ERROR_DISCONNECTED => self.reopen()?,
                e => return check(e, "write"),
            }
        }
        Ok(())
    }

    fn delay_frames(&self) -> u64 {
        // SAFETY: valid stream handle.
        let (written, read) = unsafe {
            (
                AAudioStream_getFramesWritten(self.stream),
                AAudioStream_getFramesRead(self.stream),
            )
        };
        (written - read).max(0) as u64
    }

    fn flush(&mut self) -> Result<()> {
        // AAudio flushes only a paused stream.
        // SAFETY: valid stream handle.
        unsafe {
            check(AAudioStream_requestPause(self.stream), "pause")?;
            self.wait_for(AAUDIO_STREAM_STATE_PAUSED)?;
            check(AAudioStream_requestFlush(self.stream), "flush")?;
            self.wait_for(AAUDIO_STREAM_STATE_FLUSHED)?;
            check(AAudioStream_requestStart(self.stream), "start")?;
        }
        self.paused = false;
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) -> Result<()> {
        if paused == self.paused {
            return Ok(());
        }
        self.paused = paused;
        // SAFETY: valid stream handle.
        let r = unsafe {
            if paused {
                AAudioStream_requestPause(self.stream)
            } else {
                AAudioStream_requestStart(self.stream)
            }
        };
        match r {
            AAUDIO_ERROR_DISCONNECTED if !paused => self.reopen(),
            AAUDIO_ERROR_DISCONNECTED => Ok(()),
            r => check(r, if paused { "pause" } else { "start" }),
        }
    }

    fn drain(&mut self) -> Result<()> {
        // No drain call that keeps the stream usable: wait until the device has read everything.
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.delay_frames() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn describe(&self) -> String {
        format!(
            "AAudio shared · {} Hz · {} ch · float (Android mixer)",
            self.rate, self.channels
        )
    }
}

impl Drop for AAudioOutput {
    fn drop(&mut self) {
        // SAFETY: valid stream handle, closed exactly once.
        unsafe {
            AAudioStream_requestStop(self.stream);
            AAudioStream_close(self.stream);
        }
    }
}
