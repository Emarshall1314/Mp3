//! Hybrid logical clocks: wall-clock milliseconds plus a counter, then the device, so every op
//! has a distinct timestamp, timestamps follow real time where clocks agree, and an op made after
//! seeing another always sorts after it even when the devices' clocks disagree.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A device's identity: random, made once per install.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub [u8; 16]);

impl DeviceId {
    pub fn new() -> Self {
        Self(*uuid::Uuid::now_v7().as_bytes())
    }

    pub fn parse(s: &str) -> Option<Self> {
        uuid::Uuid::try_parse(s).ok().map(|u| Self(*u.as_bytes()))
    }
}

impl Default for DeviceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", uuid::Uuid::from_bytes(self.0).simple())
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({self})")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hlc {
    /// Milliseconds since the Unix epoch, as near as the clocks allow.
    pub ms: u64,
    /// Orders ops within the same millisecond.
    pub n: u32,
    pub device: DeviceId,
}

/// A device's clock. Persisted with the replica so it never goes backwards across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    device: DeviceId,
    ms: u64,
    n: u32,
}

impl Clock {
    pub fn new(device: DeviceId) -> Self {
        Self {
            device,
            ms: 0,
            n: 0,
        }
    }

    pub fn device(&self) -> DeviceId {
        self.device
    }

    /// A new timestamp, after every one this clock has made or seen. `wall` is the time now.
    pub fn tick(&mut self, wall: u64) -> Hlc {
        if wall > self.ms {
            self.ms = wall;
            self.n = 0;
        } else {
            self.n = self.n.saturating_add(1);
        }
        Hlc {
            ms: self.ms,
            n: self.n,
            device: self.device,
        }
    }

    /// Notes another device's timestamp, so later local ops sort after it.
    pub fn observe(&mut self, seen: Hlc) {
        if (seen.ms, seen.n) > (self.ms, self.n) {
            self.ms = seen.ms;
            self.n = seen.n;
        }
    }
}

/// Milliseconds since the Unix epoch.
pub fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_increase_even_when_the_wall_clock_goes_back() {
        let mut c = Clock::new(DeviceId::new());
        let a = c.tick(1000);
        let b = c.tick(900);
        let d = c.tick(1000);
        assert!(a < b && b < d);
        assert_eq!(c.tick(2000).ms, 2000);
    }

    #[test]
    fn ops_made_after_seeing_another_sort_after_it() {
        let mut slow = Clock::new(DeviceId::new());
        let mut fast = Clock::new(DeviceId::new());
        let seen = fast.tick(5_000_000);
        slow.observe(seen);
        assert!(slow.tick(1000) > seen);
    }

    #[test]
    fn device_ids_round_trip_as_text() {
        let id = DeviceId::new();
        assert_eq!(DeviceId::parse(&id.to_string()), Some(id));
        assert_eq!(DeviceId::parse("nope"), None);
    }
}
