//! Sync between the owner's devices on the same Wi-Fi, with no server (D64): each device appends
//! its changes (ops) to its own log in a local store, copies the logs paired devices on the
//! network have that it doesn't, and merges them. Merging is deterministic, so every device that
//! has read the same ops ends in the same state whatever order they arrived in.
//!
//! - [`hlc`]: hybrid logical clocks, which order ops across devices.
//! - [`op`]: the changes that sync, keyed by audio hash so they find the same song everywhere.
//! - [`segment`]: the log file format (CBOR, zstd-compressed).
//! - [`store`]: where logs live: a folder, or memory in tests.
//! - [`peer`]: paired devices talking over the network: a small server, its client, and copying
//!   what one device's store has that another's lacks.
//! - [`discovery`]: finding paired devices on the network (mDNS).
//! - [`state`]: the merged state; last writer wins per field, and concurrent edits are kept as
//!   conflicts for the owner to review.
//! - [`replica`]: one device: records local changes, pushes and pulls.
//! - [`crypt`]: the library key, from the pairing code, which encrypts and signs all traffic.

pub mod crypt;
pub mod discovery;
pub mod hlc;
pub mod op;
pub mod peer;
pub mod replica;
pub mod segment;
pub mod state;
pub mod store;

pub use crypt::{Encryption, Key};
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
