//! The changes that sync between devices.

use serde::{Deserialize, Serialize};

use crate::hlc::Hlc;

/// A song, the same on every device: its audio hash (D5, D20), and for a song inside a cue sheet,
/// its index there. Paths and database ids differ between devices, so ops never use them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TrackKey {
    pub audio_hash: String,
    pub cue: Option<u32>,
}

impl TrackKey {
    pub fn new(audio_hash: impl Into<String>, cue: Option<u32>) -> Self {
        Self {
            audio_hash: audio_hash.into(),
            cue,
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
    pub seen: Option<Hlc>,
    pub change: Change,
}
