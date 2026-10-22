//! The player thread and the output thread.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cadenza_decode::{Bounded, Decoder, DsdToPcm, SampleFormat};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::convert::{Resampler, remix, to_f32};
use crate::output::{Output, OutputBackend};
use crate::queue::{Queue, QueueItem, Repeat};
use crate::{Cmd, Error, Event, PlayerState, Result, Status, StreamInfo};

/// How far the decoder runs ahead of the speakers.
const AHEAD: Duration = Duration::from_secs(2);
const POSITION_EVERY: Duration = Duration::from_millis(250);

/// State shared between the player thread and one output thread.
#[derive(Default)]
struct OutShared {
    /// Frames heard: popped from the ring (written or discarded) minus the device's delay.
    played: AtomicU64,
    epoch: AtomicU64,
    ack: AtomicU64,
    paused: AtomicBool,
    quit: AtomicBool,
    finished: AtomicBool,
    volume_bits: AtomicU32,
    error: Mutex<Option<String>>,
}

fn gain(volume: f32) -> f32 {
    // Perceptual curve: the slider's midpoint is about −18 dB.
    volume.clamp(0.0, 1.0).powi(3)
}

fn output_thread(mut out: Box<dyn Output>, mut ring: Consumer<f32>, sh: Arc<OutShared>) {
    let ch = out.channels().max(1) as usize;
    let period = (out.rate() as usize / 100).max(64) * ch; // ~10 ms per write
    let mut buf: Vec<f32> = Vec::with_capacity(period);
    let mut consumed: u64 = 0;
    let mut seen_epoch = 0;
    let mut paused = false;
    let mut current_gain = gain(f32::from_bits(sh.volume_bits.load(Ordering::Relaxed)));
    loop {
        if sh.quit.load(Ordering::Acquire) {
            break;
        }
        let epoch = sh.epoch.load(Ordering::Acquire);
        if epoch != seen_epoch {
            // Flush: discard everything queued, drop the device buffer, acknowledge.
            let n = ring.slots();
            if let Ok(chunk) = ring.read_chunk(n) {
                chunk.commit_all();
            }
            consumed += (n / ch) as u64;
            if let Err(e) = out.flush() {
                set_error(&sh, e);
            }
            seen_epoch = epoch;
            sh.played.store(consumed, Ordering::Release);
            sh.ack.store(epoch, Ordering::Release);
        }
        let want_pause = sh.paused.load(Ordering::Acquire);
        if want_pause != paused {
            if let Err(e) = out.set_paused(want_pause) {
                set_error(&sh, e);
            }
            paused = want_pause;
        }
        if paused {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        let available = ring.slots() / ch * ch;
        if available == 0 {
            sh.played.store(
                consumed.saturating_sub(out.delay_frames()),
                Ordering::Release,
            );
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        let n = available.min(period);
        buf.clear();
        if let Ok(chunk) = ring.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            buf.extend_from_slice(a);
            buf.extend_from_slice(b);
            chunk.commit_all();
        }
        // Volume with a per-buffer linear ramp, so changes never click.
        let target = gain(f32::from_bits(sh.volume_bits.load(Ordering::Relaxed)));
        if target != 1.0 || current_gain != 1.0 {
            let frames = buf.len() / ch;
            for (f, frame) in buf.chunks_exact_mut(ch).enumerate() {
                let g = current_gain + (target - current_gain) * (f + 1) as f32 / frames as f32;
                frame.iter_mut().for_each(|s| *s *= g);
            }
            current_gain = target;
        }
        if let Err(e) = out.write(&buf) {
            set_error(&sh, e);
            thread::sleep(Duration::from_millis(50));
        }
        consumed += (n / ch) as u64;
        sh.played.store(
            consumed.saturating_sub(out.delay_frames()),
            Ordering::Release,
        );
    }
    sh.finished.store(true, Ordering::Release);
}

fn set_error(sh: &OutShared, e: Error) {
    if let Ok(mut slot) = sh.error.lock() {
        *slot = Some(e.to_string());
    }
}

/// An open output plus its thread and ring.
struct Stream {
    ring: Producer<f32>,
    shared: Arc<OutShared>,
    thread: Option<JoinHandle<()>>,
    /// The format we asked for (the source's); the device may have granted another.
    requested: (u32, u16),
    rate: u32,
    channels: u16,
    pushed: u64,
    describe: String,
}

impl Stream {
    fn open(backend: &dyn OutputBackend, rate: u32, channels: u16, volume: f32) -> Result<Self> {
        let out = backend.open(rate, channels)?;
        let (granted_rate, granted_channels, describe) =
            (out.rate(), out.channels(), out.describe());
        let capacity =
            (granted_rate as f64 * AHEAD.as_secs_f64()) as usize * granted_channels as usize;
        let (producer, consumer) = RingBuffer::<f32>::new(capacity);
        let shared = Arc::new(OutShared::default());
        shared
            .volume_bits
            .store(volume.to_bits(), Ordering::Relaxed);
        let sh = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("cadenza-output".into())
            .spawn(move || output_thread(out, consumer, sh))
            .map_err(|e| Error::Output(e.to_string()))?;
        Ok(Self {
            ring: producer,
            shared,
            thread: Some(thread),
            requested: (rate, channels),
            rate: granted_rate,
            channels: granted_channels,
            pushed: 0,
            describe,
        })
    }

    fn played(&self) -> u64 {
        self.shared.played.load(Ordering::Acquire)
    }

    fn space_frames(&self) -> usize {
        self.ring.slots() / self.channels.max(1) as usize
    }

    /// Pushes as much of `samples` as fits; returns samples consumed.
    fn push(&mut self, samples: &[f32]) -> usize {
        let ch = self.channels.max(1) as usize;
        let n = (self.ring.slots() / ch * ch).min(samples.len() / ch * ch);
        if n == 0 {
            return 0;
        }
        if let Ok(mut chunk) = self.ring.write_chunk(n) {
            let (a, b) = chunk.as_mut_slices();
            a.copy_from_slice(&samples[..a.len()]);
            b.copy_from_slice(&samples[a.len()..n]);
            chunk.commit_all();
        }
        self.pushed += (n / ch) as u64;
        n
    }

    /// Discards everything not yet heard; returns once the output thread confirmed.
    fn flush(&mut self) {
        let epoch = self.shared.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.shared.ack.load(Ordering::Acquire) < epoch
            && !self.shared.finished.load(Ordering::Acquire)
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_micros(500));
        }
        self.shared.played.store(self.pushed, Ordering::Release);
    }

    fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Release);
    }

    fn set_volume(&self, v: f32) {
        self.shared
            .volume_bits
            .store(v.to_bits(), Ordering::Relaxed);
    }

    fn take_error(&self) -> Option<String> {
        self.shared.error.lock().ok().and_then(|mut e| e.take())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A track being decoded.
struct Source {
    dec: Box<dyn Decoder>,
    index: usize,
    item: QueueItem,
    format: SampleFormat,
    rate: u32,
    channels: u16,
    duration_ms: u64,
    info: StreamInfo,
}

fn open_source(item: &QueueItem, index: usize) -> Result<Source> {
    let mut dec = cadenza_decode::open(&item.path)?;
    if item.start_frame.is_some() || item.end_frame.is_some() {
        dec = Box::new(Bounded::new(
            dec,
            item.start_frame.unwrap_or(0),
            item.end_frame,
        )?);
    }
    let original = dec.spec().clone();
    if original.format == SampleFormat::Dsd {
        dec = Box::new(DsdToPcm::new(dec)?);
    }
    let spec = dec.spec().clone();
    let duration_ms = spec
        .total_frames
        .map(|f| f * 1000 / spec.sample_rate.max(1) as u64)
        .unwrap_or(0);
    let info = StreamInfo {
        codec: original.codec.to_owned(),
        lossless: original.lossless,
        sample_rate: original.sample_rate,
        bits: match original.format {
            SampleFormat::Int(b) if original.lossless => Some(b),
            _ => None,
        },
        float: original.format == SampleFormat::Float && original.lossless,
        dsd_rate: (original.format == SampleFormat::Dsd).then_some(original.sample_rate),
        channels: original.channels,
        output: String::new(),
        resampled: false,
    };
    Ok(Source {
        format: spec.format,
        rate: spec.sample_rate,
        channels: spec.channels,
        dec,
        index,
        item: item.clone(),
        duration_ms,
        info,
    })
}

/// A stretch of the output stream belonging to one queue entry.
#[derive(Debug, Clone)]
struct Segment {
    /// Stream frame (output rate) where this entry's audio starts.
    start: u64,
    index: usize,
    track_id: i64,
    /// Position within the track at `start`, in ms (non-zero after a seek).
    offset_ms: u64,
    duration_ms: u64,
    rate: u32,
    info: StreamInfo,
}

struct Player {
    backend: Box<dyn OutputBackend>,
    queue: Queue,
    status: Status,
    volume: f32,
    stream: Option<Stream>,
    source: Option<Source>,
    /// Decoded next track whose format needs a new output (waits for the ring to drain).
    reopen_with: Option<Source>,
    resampler: Option<Resampler>,
    resampled_out: u64,
    pending: Vec<f32>,
    segments: VecDeque<Segment>,
    /// No more audio to decode; stop when everything queued has been heard.
    ending: bool,
    state: Arc<Mutex<PlayerState>>,
    queue_ids: Arc<Mutex<Vec<i64>>>,
    queue_version: u64,
    published_queue: Vec<i64>,
    emit: Box<dyn Fn(Event) + Send + Sync>,
    last_position: Instant,
    f32_buf: Vec<f32>,
    remix_buf: Vec<f32>,
}

pub(crate) fn run(
    rx: Receiver<Cmd>,
    backend: Box<dyn OutputBackend>,
    state: Arc<Mutex<PlayerState>>,
    queue_ids: Arc<Mutex<Vec<i64>>>,
    emit: Box<dyn Fn(Event) + Send + Sync>,
) {
    let mut p = Player {
        backend,
        queue: Queue::default(),
        status: Status::Stopped,
        volume: 1.0,
        stream: None,
        source: None,
        reopen_with: None,
        resampler: None,
        resampled_out: 0,
        pending: Vec::new(),
        segments: VecDeque::new(),
        ending: false,
        state,
        queue_ids,
        queue_version: 0,
        published_queue: Vec::new(),
        emit,
        last_position: Instant::now(),
        f32_buf: Vec::new(),
        remix_buf: Vec::new(),
    };
    loop {
        let busy = p.status == Status::Playing && p.stream.is_some();
        let cmd = if busy {
            match rx.recv_timeout(Duration::from_millis(if p.wants_data() { 0 } else { 5 })) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        if let Some(cmd) = cmd {
            if matches!(cmd, Cmd::Quit) {
                p.stream = None;
                return;
            }
            p.handle(cmd);
        }
        if p.status == Status::Playing {
            p.fill();
        }
        p.bookkeeping();
    }
}

impl Player {
    fn wants_data(&self) -> bool {
        self.source.is_some()
            && self.reopen_with.is_none()
            && self
                .stream
                .as_ref()
                .is_some_and(|s| s.space_frames() > (s.rate as usize / 20))
    }

    fn snapshot(&self) -> PlayerState {
        let seg = self.segments.front();
        PlayerState {
            status: self.status,
            queue_length: self.queue.items.len(),
            queue_version: self.queue_version,
            index: self.queue.current,
            track_id: self.queue.current_item().map(|i| i.track_id),
            position_ms: self.position_ms(),
            duration_ms: seg.map(|s| s.duration_ms).unwrap_or(0),
            volume: self.volume,
            repeat: self.queue.repeat,
            shuffle: self.queue.shuffle,
            stream: self.current_info(),
        }
    }

    fn current_info(&self) -> Option<StreamInfo> {
        let stream = self.stream.as_ref()?;
        let seg = self.segments.front()?;
        let mut info = seg.info.clone();
        info.output = stream.describe.clone();
        info.resampled = stream.rate != info.sample_rate && info.dsd_rate.is_none();
        Some(info)
    }

    fn publish(&mut self) {
        // Ship the (possibly 200 000-entry) id list only when it changed, and only to the
        // shared vector: state events carry its length and version.
        let ids: Vec<i64> = self.queue.items.iter().map(|i| i.track_id).collect();
        if ids != self.published_queue {
            self.queue_version += 1;
            if let Ok(mut q) = self.queue_ids.lock() {
                q.clone_from(&ids);
            }
            self.published_queue = ids;
        }
        let snap = self.snapshot();
        if let Ok(mut s) = self.state.lock() {
            *s = snap.clone();
        }
        (self.emit)(Event::State(snap));
    }

    fn position_ms(&self) -> u64 {
        match (self.segments.front(), self.stream.as_ref()) {
            (Some(seg), Some(stream)) => {
                let frames = stream.played().saturating_sub(seg.start);
                (seg.offset_ms + frames * 1000 / seg.rate.max(1) as u64).min(seg.duration_ms.max(1))
            }
            (Some(seg), None) => seg.offset_ms,
            _ => 0,
        }
    }

    fn finish_current(&mut self, completed: bool) {
        if let Some(seg) = self.segments.front() {
            let played = self.position_ms().saturating_sub(seg.offset_ms);
            let completed = completed || played >= (seg.duration_ms / 2).min(240_000);
            (self.emit)(Event::TrackFinished {
                track_id: seg.track_id,
                played_ms: played,
                completed,
            });
        }
    }

    /// Hard start: drops whatever is queued and starts `index` at `offset_ms`.
    fn start_at(&mut self, index: usize, offset_ms: u64) {
        let mut index = index;
        let mut tries = 0;
        loop {
            let Some(item) = self.queue.items.get(index).cloned() else {
                self.stop_output();
                return;
            };
            match open_source(&item, index) {
                Ok(mut src) => {
                    let frame = offset_ms * src.rate as u64 / 1000;
                    if frame > 0
                        && let Err(e) = src.dec.seek(frame)
                    {
                        (self.emit)(Event::Error {
                            track_id: Some(item.track_id),
                            message: e.to_string(),
                        });
                    }
                    if let Err(e) = self.ensure_stream(src.rate, src.channels, true) {
                        (self.emit)(Event::Error {
                            track_id: Some(item.track_id),
                            message: e.to_string(),
                        });
                        self.stop_output();
                        return;
                    }
                    self.queue.current = Some(index);
                    let stream = self
                        .stream
                        .as_ref()
                        .map(|s| (s.pushed, s.rate))
                        .unwrap_or((0, src.rate));
                    self.segments.clear();
                    self.segments.push_back(Segment {
                        start: stream.0,
                        index,
                        track_id: item.track_id,
                        offset_ms,
                        duration_ms: src.duration_ms,
                        rate: stream.1,
                        info: src.info.clone(),
                    });
                    self.source = Some(src);
                    self.reopen_with = None;
                    self.ending = false;
                    if self.status == Status::Stopped {
                        self.status = Status::Playing;
                    }
                    if let Some(s) = &self.stream {
                        s.set_paused(self.status == Status::Paused);
                    }
                    self.publish();
                    return;
                }
                Err(e) => {
                    (self.emit)(Event::Error {
                        track_id: Some(item.track_id),
                        message: e.to_string(),
                    });
                    tries += 1;
                    match self
                        .queue
                        .after(index)
                        .filter(|n| *n != index && tries < self.queue.items.len())
                    {
                        Some(n) => index = n,
                        None => {
                            self.stop_output();
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Makes sure an output matching (rate, channels) is open and empty.
    fn ensure_stream(&mut self, rate: u32, channels: u16, flush: bool) -> Result<()> {
        let reuse = self
            .stream
            .as_ref()
            .is_some_and(|s| s.requested == (rate, channels));
        if reuse {
            if flush && let Some(s) = self.stream.as_mut() {
                s.flush();
            }
        } else {
            self.stream = None;
            self.stream = Some(Stream::open(
                self.backend.as_ref(),
                rate,
                channels,
                self.volume,
            )?);
        }
        self.pending.clear();
        self.resampled_out = 0;
        let stream = self.stream.as_ref().ok_or(Error::Stopped)?;
        self.resampler = if stream.rate != rate {
            Some(Resampler::new(rate, stream.rate, stream.channels)?)
        } else {
            None
        };
        Ok(())
    }

    fn stop_output(&mut self) {
        self.stream = None;
        self.source = None;
        self.reopen_with = None;
        self.resampler = None;
        self.pending.clear();
        self.ending = false;
        self.status = Status::Stopped;
        if let Some(seg) = self.segments.front_mut() {
            seg.offset_ms = 0;
            seg.start = 0;
        }
        self.publish();
    }

    /// Decodes into the ring until it is full or the source is exhausted.
    fn fill(&mut self) {
        loop {
            let Some(stream) = self.stream.as_mut() else {
                return;
            };
            if !self.pending.is_empty() {
                let n = stream.push(&self.pending);
                self.pending.drain(..n);
                if !self.pending.is_empty() {
                    return;
                }
            }
            if self.reopen_with.is_some()
                || self.ending
                || stream.space_frames() < stream.rate as usize / 50
            {
                return;
            }
            let Some(src) = self.source.as_mut() else {
                return;
            };
            let (out_rate, out_ch) = (stream.rate, stream.channels);
            match src.dec.next_chunk() {
                Ok(Some(chunk)) => {
                    to_f32(chunk, src.format, &mut self.f32_buf);
                    remix(&self.f32_buf, src.channels, out_ch, &mut self.remix_buf);
                    match self.resampler.as_mut() {
                        Some(r) => {
                            let before = self.pending.len();
                            if let Err(e) = r.process(&self.remix_buf, &mut self.pending) {
                                (self.emit)(Event::Error {
                                    track_id: Some(src.item.track_id),
                                    message: e.to_string(),
                                });
                            }
                            self.resampled_out +=
                                ((self.pending.len() - before) / out_ch.max(1) as usize) as u64;
                        }
                        None => self.pending.extend_from_slice(&self.remix_buf),
                    }
                }
                Ok(None) => self.source_ended(out_rate, out_ch),
                Err(e) => {
                    (self.emit)(Event::Error {
                        track_id: Some(src.item.track_id),
                        message: e.to_string(),
                    });
                    self.source_ended(out_rate, out_ch);
                }
            }
        }
    }

    /// The decoder hit the end: chain the next entry gaplessly, or wait to reopen, or end.
    fn source_ended(&mut self, out_rate: u32, out_ch: u16) {
        let Some(src) = self.source.take() else {
            return;
        };
        let next = self.queue.after(src.index);
        let mut next_src = None;
        if let Some(mut n) = next {
            let mut tries = 0;
            while tries < self.queue.items.len() {
                let Some(item) = self.queue.items.get(n).cloned() else {
                    break;
                };
                match open_source(&item, n) {
                    Ok(s) => {
                        next_src = Some(s);
                        break;
                    }
                    Err(e) => {
                        (self.emit)(Event::Error {
                            track_id: Some(item.track_id),
                            message: e.to_string(),
                        });
                        tries += 1;
                        match self.queue.after(n) {
                            Some(m) if m != n => n = m,
                            _ => break,
                        }
                    }
                }
            }
        }
        let same_format = next_src
            .as_ref()
            .is_some_and(|n| (n.rate, n.channels) == (src.rate, src.channels));
        if !same_format && let Some(r) = self.resampler.as_mut() {
            // Close the resampled stream exactly: total out = total in × ratio.
            let expected = src
                .dec
                .spec()
                .total_frames
                .map(|f| f * r.to as u64 / r.from as u64);
            let _ = r.flush(&mut self.pending, expected, self.resampled_out);
        }
        match next_src {
            Some(n) if same_format => {
                let stream = self
                    .stream
                    .as_ref()
                    .map(|s| (s.pushed, s.rate))
                    .unwrap_or((0, out_rate));
                let pending_frames = (self.pending.len() / out_ch.max(1) as usize) as u64;
                self.segments.push_back(Segment {
                    start: stream.0 + pending_frames,
                    index: n.index,
                    track_id: n.item.track_id,
                    offset_ms: 0,
                    duration_ms: n.duration_ms,
                    rate: stream.1,
                    info: n.info.clone(),
                });
                self.source = Some(n);
            }
            Some(n) => self.reopen_with = Some(n),
            None => self.ending = true,
        }
    }

    fn bookkeeping(&mut self) {
        let Some(stream) = self.stream.as_ref() else {
            return;
        };
        if let Some(e) = stream.take_error() {
            (self.emit)(Event::Error {
                track_id: self.segments.front().map(|s| s.track_id),
                message: e,
            });
        }
        let played = stream.played();
        let pushed = stream.pushed + (self.pending.len() / stream.channels.max(1) as usize) as u64;
        let mut changed = false;
        while self.segments.len() > 1 && played >= self.segments[1].start {
            self.finish_current(true);
            self.segments.pop_front();
            self.queue.current = self.segments.front().map(|s| s.index);
            changed = true;
        }
        let all_heard = played >= pushed && self.pending.is_empty();
        if all_heard && self.reopen_with.is_some() {
            self.finish_current(true);
            let next = self.reopen_with.take();
            if let Some(n) = next {
                self.stream = None;
                match self.ensure_stream(n.rate, n.channels, false) {
                    Ok(()) => {
                        let rate = self.stream.as_ref().map(|s| s.rate).unwrap_or(n.rate);
                        self.segments.clear();
                        self.segments.push_back(Segment {
                            start: 0,
                            index: n.index,
                            track_id: n.item.track_id,
                            offset_ms: 0,
                            duration_ms: n.duration_ms,
                            rate,
                            info: n.info.clone(),
                        });
                        self.queue.current = Some(n.index);
                        self.source = Some(n);
                        if let Some(s) = &self.stream {
                            s.set_paused(self.status == Status::Paused);
                        }
                    }
                    Err(e) => {
                        (self.emit)(Event::Error {
                            track_id: Some(n.item.track_id),
                            message: e.to_string(),
                        });
                        self.stop_output();
                        return;
                    }
                }
            }
            changed = true;
        } else if all_heard && self.ending && self.segments.len() <= 1 {
            self.finish_current(true);
            self.stop_output();
            return;
        }
        if changed {
            self.publish();
        }
        if self.status == Status::Playing && self.last_position.elapsed() >= POSITION_EVERY {
            self.last_position = Instant::now();
            let position_ms = self.position_ms();
            if let Ok(mut s) = self.state.lock() {
                s.position_ms = position_ms;
            }
            if let Some(seg) = self.segments.front() {
                (self.emit)(Event::Position {
                    track_id: seg.track_id,
                    position_ms,
                });
            }
        }
    }

    /// After queue edits: if audio for the wrong next entry is already queued, restart cleanly
    /// at the current position.
    fn resync_after_queue_change(&mut self) {
        let Some(front) = self.segments.front().cloned() else {
            self.publish();
            return;
        };
        let Some(current) = self.queue.current else {
            self.stop_output();
            return;
        };
        let expected_next = self
            .queue
            .after(current)
            .and_then(|i| self.queue.items.get(i));
        let prepared_next = self
            .segments
            .get(1)
            .map(|s| s.track_id)
            .or(self.reopen_with.as_ref().map(|s| s.item.track_id))
            .or_else(|| {
                self.source
                    .as_ref()
                    .filter(|s| s.item.track_id != front.track_id)
                    .map(|s| s.item.track_id)
            });
        let stale = match prepared_next {
            Some(id) => expected_next.map(|i| i.track_id) != Some(id),
            None => self.ending && expected_next.is_some(),
        };
        if stale && self.stream.is_some() {
            let pos = self.position_ms();
            self.start_at(current, pos);
        } else {
            // Indices may have shifted; keep segment indices in step.
            if let Some(seg) = self.segments.front_mut() {
                seg.index = current;
            }
            if let (Some(seg), Some(i)) = (self.segments.get_mut(1), self.queue.after(current)) {
                seg.index = i;
            }
            if let Some(src) = self.source.as_mut() {
                src.index = if src.item.track_id == front.track_id {
                    current
                } else {
                    self.queue.after(current).unwrap_or(current)
                };
            }
            self.publish();
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::SetQueue { items, start, play } => {
                if self.stream.is_some() {
                    self.finish_current(false);
                }
                self.queue.set(items, start);
                self.segments.clear();
                if play && let Some(i) = self.queue.current {
                    self.status = Status::Playing;
                    self.start_at(i, 0);
                } else {
                    self.stop_output();
                }
            }
            Cmd::Play => match self.status {
                Status::Paused => {
                    self.status = Status::Playing;
                    if let Some(s) = &self.stream {
                        s.set_paused(false);
                    }
                    self.publish();
                }
                Status::Stopped => {
                    if let Some(i) = self.queue.current {
                        self.status = Status::Playing;
                        self.start_at(i, 0);
                    }
                }
                Status::Playing => {}
            },
            Cmd::Pause => {
                if self.status == Status::Playing {
                    self.status = Status::Paused;
                    if let Some(s) = &self.stream {
                        s.set_paused(true);
                    }
                    self.publish();
                }
            }
            Cmd::Toggle => {
                let next = if self.status == Status::Playing {
                    Cmd::Pause
                } else {
                    Cmd::Play
                };
                self.handle(next);
            }
            Cmd::Stop => {
                self.finish_current(false);
                self.stop_output();
            }
            Cmd::Next => {
                let Some(cur) = self.queue.current else {
                    return;
                };
                self.finish_current(false);
                // Skipping moves on even with Repeat One.
                let next = if cur + 1 < self.queue.items.len() {
                    Some(cur + 1)
                } else if self.queue.repeat != Repeat::Off {
                    Some(0)
                } else {
                    None
                };
                match next {
                    Some(n) => self.start_at(n, 0),
                    None => {
                        self.queue.current = Some(cur);
                        self.stop_output();
                    }
                }
            }
            Cmd::Previous => {
                let Some(cur) = self.queue.current else {
                    return;
                };
                if self.position_ms() > 3000 || self.queue.before(cur).is_none() {
                    self.start_at(cur, 0);
                } else if let Some(prev) = self.queue.before(cur) {
                    self.finish_current(false);
                    self.start_at(prev, 0);
                }
            }
            Cmd::PlayIndex(i) => {
                if i < self.queue.items.len() {
                    self.finish_current(false);
                    if self.status == Status::Paused {
                        self.status = Status::Playing;
                    }
                    self.status = Status::Playing;
                    self.start_at(i, 0);
                }
            }
            Cmd::Seek(ms) => {
                if let Some(seg) = self.segments.front().cloned() {
                    let ms = ms.min(seg.duration_ms.saturating_sub(1));
                    self.start_at(seg.index, ms);
                }
            }
            Cmd::Volume(v) => {
                self.volume = v;
                if let Some(s) = &self.stream {
                    s.set_volume(v);
                }
                self.publish();
            }
            Cmd::InsertNext(items) => {
                self.queue.insert_next(items);
                self.resync_after_queue_change();
            }
            Cmd::Append(items) => {
                self.queue.append(items);
                self.resync_after_queue_change();
            }
            Cmd::Remove(i) => {
                let was_current = self.queue.remove(i);
                if was_current {
                    self.finish_current(false);
                    match (self.queue.current, self.status) {
                        (Some(c), Status::Playing | Status::Paused) => self.start_at(c, 0),
                        _ => self.stop_output(),
                    }
                } else {
                    self.resync_after_queue_change();
                }
            }
            Cmd::Move(from, to) => {
                self.queue.move_item(from, to);
                self.resync_after_queue_change();
            }
            Cmd::ClearUpcoming => {
                self.queue.clear_upcoming();
                self.resync_after_queue_change();
            }
            Cmd::Repeat(r) => {
                self.queue.repeat = r;
                self.resync_after_queue_change();
            }
            Cmd::Shuffle(on) => {
                self.queue.set_shuffle(on);
                self.resync_after_queue_change();
            }
            Cmd::Quit => {}
        }
    }
}
