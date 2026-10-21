//! A frame range of another stream: CUE-sheet tracks inside a single-file album.

use crate::{Chunk, Decoder, Result, StreamSpec};

pub struct Bounded {
    inner: Box<dyn Decoder>,
    spec: StreamSpec,
    start: u64,
    pos: u64,
    len: u64,
}

impl std::fmt::Debug for Bounded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bounded")
            .field("start", &self.start)
            .field("len", &self.len)
            .finish()
    }
}

impl Bounded {
    /// Plays `[start, end)` of `inner` (in its frames); `end = None` means to the end.
    pub fn new(mut inner: Box<dyn Decoder>, start: u64, end: Option<u64>) -> Result<Self> {
        let total = inner.spec().total_frames;
        let end = end.or(total).unwrap_or(u64::MAX);
        let mut spec = inner.spec().clone();
        spec.total_frames = Some(end.saturating_sub(start));
        if start > 0 {
            inner.seek(start)?;
        }
        Ok(Self {
            inner,
            spec,
            start,
            pos: 0,
            len: end.saturating_sub(start),
        })
    }
}

impl Decoder for Bounded {
    fn spec(&self) -> &StreamSpec {
        &self.spec
    }

    fn next_chunk(&mut self) -> Result<Option<Chunk<'_>>> {
        let remaining = self.len - self.pos;
        if remaining == 0 {
            return Ok(None);
        }
        let ch = self.spec.channels;
        let Some(chunk) = self.inner.next_chunk()? else {
            return Ok(None);
        };
        let frames = chunk.frames(ch) as u64;
        let take = frames.min(remaining);
        self.pos += take;
        Ok(Some(chunk.first_frames(take as usize, ch)))
    }

    fn seek(&mut self, frame: u64) -> Result<u64> {
        let reached = self.inner.seek(self.start + frame.min(self.len))?;
        self.pos = reached.saturating_sub(self.start);
        Ok(self.pos)
    }
}

impl<'a> Chunk<'a> {
    /// The first `frames` frames of this chunk.
    pub fn first_frames(self, frames: usize, channels: u16) -> Chunk<'a> {
        let ch = channels.max(1) as usize;
        match self {
            Chunk::Int(s) => Chunk::Int(&s[..(frames * ch).min(s.len())]),
            Chunk::Float(s) => Chunk::Float(&s[..(frames * ch).min(s.len())]),
            Chunk::Dsd(s) => Chunk::Dsd(&s[..(frames.div_ceil(8) * ch).min(s.len())]),
        }
    }
}
