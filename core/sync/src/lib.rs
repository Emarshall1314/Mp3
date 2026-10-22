//! Sync between the owner's devices with no server: each device appends its changes (ops) to its
//! own log in a shared store (a folder another tool keeps in step, or later a cloud drive), reads
//! the other devices' logs, and merges them. Merging is deterministic, so every device that has
//! read the same ops ends in the same state whatever order they arrived in.
//!
//! - [`hlc`]: hybrid logical clocks, which order ops across devices.
//! - [`op`]: the changes that sync, keyed by audio hash so they find the same song everywhere.
//! - [`segment`]: the log file format (CBOR, zstd-compressed).
//! - [`store`]: where logs live: a folder, or memory in tests.
//! - [`state`]: the merged state; last writer wins per field, and concurrent edits are kept as
//!   conflicts for the owner to review.
//! - [`replica`]: one device: records local changes, pushes and pulls.
//! - [`crypt`]: optional encryption of everything in the store.

pub mod crypt;
pub mod hlc;
pub mod op;
pub mod replica;
pub mod segment;
pub mod state;
pub mod store;

pub use crypt::{EncryptedStore, Encryption, Key, LibraryInfo};
pub use hlc::{Clock, DeviceId, Hlc};
pub use op::{Change, Op, TrackKey};
pub use replica::{Replica, SyncReport};
pub use state::{Conflict, Effect, State};
pub use store::{FolderStore, MemStore, Store};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a Cadenza log segment: {0}")]
    Segment(String),
    #[error("{0}")]
    Crypto(String),
    #[error("bad path in store: {0}")]
    Path(String),
    #[error("couldn't encode: {0}")]
    Encode(String),
}

pub type Result<T> = std::result::Result<T, Error>;
