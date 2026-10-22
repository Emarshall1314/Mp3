//! One device's side of sync. Local changes are merged at once and queued; `push` writes the
//! queue to this device's log as a new segment, and `pull` reads segments other devices have
//! written since last time. Store layout:
//!
//! ```text
//! devices/<device>.json        name, platform, last pushed
//! log/<device>/<seq>.seg       that device's ops, one segment per push
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::hlc::{Clock, DeviceId};
use crate::op::{Change, Op};
use crate::segment::{self, Segment};
use crate::state::{Effect, State};
use crate::store::Store;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replica {
    clock: Clock,
    state: State,
    /// Local ops not pushed yet.
    outbox: Vec<Op>,
    /// The number the next segment this device pushes gets.
    next_seq: u64,
    /// For each other device, the number of the next of its segments to read.
    cursors: BTreeMap<DeviceId, u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    pub pushed_ops: usize,
    pub read_ops: usize,
    /// Segments that couldn't be read (damaged, or from a newer Cadenza); skipped for now and
    /// tried again next time.
    pub unreadable: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub platform: String,
    /// Wall-clock milliseconds of its last push.
    pub last_push_ms: u64,
}

impl Replica {
    pub fn new(device: DeviceId) -> Self {
        Self {
            clock: Clock::new(device),
            state: State::new(),
            outbox: Vec::new(),
            next_seq: 0,
            cursors: BTreeMap::new(),
        }
    }

    pub fn device(&self) -> DeviceId {
        self.clock.device()
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn pending(&self) -> usize {
        self.outbox.len()
    }

    /// Local ops not pushed yet.
    pub fn outbox(&self) -> &[Op] {
        &self.outbox
    }

    /// Records a change made on this device. `wall` is the time now, in ms.
    pub fn record(&mut self, change: Change, wall: u64) -> Op {
        self.record_seen(change, wall, false)
    }

    /// Resolves a conflict: `change` replaces every version of its value, including the ones
    /// that lost to concurrent edits, so the conflict goes away on every device.
    pub fn resolve(&mut self, change: Change, wall: u64) -> Op {
        self.record_seen(change, wall, true)
    }

    fn record_seen(&mut self, change: Change, wall: u64, all: bool) -> Op {
        let op = Op {
            hlc: self.clock.tick(wall),
            seen: self.state.current(&change, all),
            change,
        };
        self.state.apply(&op);
        self.outbox.push(op.clone());
        op
    }

    /// Writes queued changes to this device's log.
    pub fn push(
        &mut self,
        store: &dyn Store,
        info: Option<(&str, &str)>,
        wall: u64,
    ) -> Result<usize> {
        let count = self.outbox.len();
        if count > 0 {
            let segment = Segment {
                device: self.device(),
                seq: self.next_seq,
                ops: self.outbox.clone(),
            };
            let path = format!("log/{}/{}", self.device(), segment::name(self.next_seq));
            store.put(&path, &segment::encode(&segment)?)?;
            self.next_seq += 1;
            self.outbox.clear();
        }
        if let Some((name, platform)) = info {
            let device = DeviceInfo {
                id: self.device().to_string(),
                name: name.to_owned(),
                platform: platform.to_owned(),
                last_push_ms: wall,
            };
            let json =
                serde_json::to_vec_pretty(&device).map_err(|e| Error::Encode(e.to_string()))?;
            store.put(&format!("devices/{}.json", self.device()), &json)?;
        }
        Ok(count)
    }

    /// Reads other devices' new segments. Calls `effect` for each change to apply locally.
    pub fn pull(
        &mut self,
        store: &dyn Store,
        mut effect: impl FnMut(Effect),
    ) -> Result<SyncReport> {
        let mut report = SyncReport::default();
        for dir in store.list("log")? {
            let Some(device) = DeviceId::parse(&dir) else {
                continue;
            };
            if device == self.device() {
                continue;
            }
            let mut next = self.cursors.get(&device).copied().unwrap_or(0);
            let mut seqs: Vec<u64> = store
                .list(&format!("log/{dir}"))?
                .iter()
                .filter_map(|n| segment::parse_name(n))
                .filter(|s| *s >= next)
                .collect();
            seqs.sort_unstable();
            for seq in seqs {
                // Segments are read in order; a gap means the store hasn't got them all yet
                // (another tool still copying), so stop there and come back.
                if seq != next {
                    break;
                }
                let path = format!("log/{dir}/{}", segment::name(seq));
                let Some(bytes) = store.get(&path)? else {
                    break;
                };
                let segment = match segment::decode(&bytes) {
                    Ok(s) if s.device == device && s.seq == seq => s,
                    Ok(_) => {
                        report.unreadable.push(format!("{path}: belongs elsewhere"));
                        break;
                    }
                    Err(e) => {
                        report.unreadable.push(format!("{path}: {e}"));
                        break;
                    }
                };
                for op in &segment.ops {
                    if op.hlc.device != device {
                        continue;
                    }
                    self.clock.observe(op.hlc);
                    if let Some(e) = self.state.apply(op) {
                        effect(e);
                    }
                    report.read_ops += 1;
                }
                next = seq + 1;
            }
            self.cursors.insert(device, next);
        }
        Ok(report)
    }

    /// Pushes, then pulls.
    pub fn sync(
        &mut self,
        store: &dyn Store,
        info: Option<(&str, &str)>,
        wall: u64,
        effect: impl FnMut(Effect),
    ) -> Result<SyncReport> {
        let pushed = self.push(store, info, wall)?;
        let mut report = self.pull(store, effect)?;
        report.pushed_ops = pushed;
        Ok(report)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        ciborium::into_writer(self, &mut out).map_err(|e| Error::Encode(e.to_string()))?;
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ciborium::from_reader(bytes).map_err(|e| Error::Segment(e.to_string()))
    }
}
