//! The changes that sync between devices.

use serde::{Deserialize, Serialize};

use crate::hlc::Hlc;

/// A song, the same on every device: its audio hash as the library stores it (D5, D20; a song
/// from a cue sheet has its index appended). Paths and database ids differ between devices, so
/// ops never use them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TrackKey {
    pub audio_hash: String,
}

impl TrackKey {
    pub fn new(audio_hash: impl Into<String>) -> Self {
        Self {
            audio_hash: audio_hash.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Change {
    /// A tag field's values (empty: removed). Last writer wins per field.
    SetField {
        track: TrackKey,
        field: String,
        values: Vec<String>,
    },
    SetFavourite {
        track: TrackKey,
        on: bool,
    },
    /// Moved to (or back out of) the trash.
    Trash {
        track: TrackKey,
        trashed: bool,
    },
    /// A device has the song's audio in the store's blobs, for devices that don't have it.
    /// The first device to share a song decides where it goes on the others.
    Shared {
        track: TrackKey,
        /// Where it is under its music folder, with `/` between parts.
        rel_path: String,
        size: u64,
    },
    /// A listen. These only accumulate, so play counts add up across devices.
    Play {
        track: TrackKey,
        /// Seconds since the Unix epoch.
        at: i64,
        played_ms: i64,
        completed: bool,
    },
}

impl Change {
    pub fn track(&self) -> &TrackKey {
        match self {
            Change::SetField { track, .. }
            | Change::SetFavourite { track, .. }
            | Change::Trash { track, .. }
            | Change::Shared { track, .. }
            | Change::Play { track, .. } => track,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    pub hlc: Hlc,
    /// For a value change: the timestamp of the value the device had when it made the change
    /// (none if it had none). An op that saw another replaced it on purpose; ops that didn't see
    /// each other were concurrent, and the one that loses is kept as a conflict.
    /// More than one when the owner resolved a conflict: the edit replaces every version listed.
    pub seen: Vec<Hlc>,
    pub change: Change,
}
