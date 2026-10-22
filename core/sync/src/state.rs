//! The merged state of every op a device has read.
//!
//! Each value (a tag field, a favourite, being in the trash) is a register that keeps every
//! version written to it. The newest version by HLC wins. A version that no later edit replaced on
//! purpose (no op names it as `seen`) was lost to a concurrent edit; if its value differs from the
//! winner it is a conflict, kept for the owner to review. All of this depends only on which ops
//! have been read, never their order, so devices that have read the same ops agree.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::hlc::Hlc;
use crate::op::{Change, Op, TrackKey};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Version<T> {
    seen: Vec<Hlc>,
    value: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Register<T> {
    versions: BTreeMap<Hlc, Version<T>>,
}

impl<T: Clone + PartialEq> Register<T> {
    fn new() -> Self {
        Self {
            versions: BTreeMap::new(),
        }
    }

    /// Adds a version; true if it is the new winner.
    fn insert(&mut self, hlc: Hlc, seen: Vec<Hlc>, value: T) -> bool {
        if self.versions.contains_key(&hlc) {
            return false;
        }
        self.versions.insert(hlc, Version { seen, value });
        self.versions
            .last_key_value()
            .is_some_and(|(k, _)| *k == hlc)
    }

    fn winner(&self) -> Option<(Hlc, &T)> {
        self.versions.last_key_value().map(|(k, v)| (*k, &v.value))
    }

    /// Versions lost to concurrent edits whose values differ from the winner's.
    fn lost(&self) -> impl Iterator<Item = (Hlc, &T)> {
        let winner = self.winner();
        self.versions.iter().filter_map(move |(hlc, v)| {
            let (w, value) = winner?;
            let replaced = self.versions.values().any(|o| o.seen.contains(hlc));
            (*hlc != w && !replaced && v.value != *value).then_some((*hlc, &v.value))
        })
    }
}

/// What a newly read op changed, for the library to apply to its own copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    Field {
        track: TrackKey,
        field: String,
        values: Vec<String>,
    },
    Favourite {
        track: TrackKey,
        on: bool,
    },
    Trash {
        track: TrackKey,
        trashed: bool,
    },
    Play {
        track: TrackKey,
        at: i64,
        played_ms: i64,
        completed: bool,
        /// The op's timestamp as text: the same on every device, so a listen is counted once.
        id: String,
    },
    /// A song's audio became available in the store.
    Shared {
        track: TrackKey,
        rel_path: String,
        size: u64,
    },
}

/// A song's audio in the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedAudio {
    pub rel_path: String,
    pub size: u64,
}

/// A listen as synced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayRecord {
    pub track: TrackKey,
    pub at: i64,
    pub played_ms: i64,
    pub completed: bool,
}

/// An edit that lost to a concurrent edit on another device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub track: TrackKey,
    /// A tag field, or "favourite" or "trash".
    pub field: String,
    pub lost_at: Hlc,
    pub lost: Vec<String>,
    pub kept_at: Hlc,
    pub kept: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct State {
    fields: BTreeMap<TrackKey, BTreeMap<String, Register<Vec<String>>>>,
    favourites: BTreeMap<TrackKey, Register<bool>>,
    trash: BTreeMap<TrackKey, Register<bool>>,
    /// Grow-only: keyed by the op's timestamp, which is unique.
    plays: BTreeMap<Hlc, PlayRecord>,
    /// The first device to share each song's audio wins; others' shares are the same audio.
    shared: BTreeMap<TrackKey, (Hlc, SharedAudio)>,
}

/// A listen's id: its op's timestamp as text.
pub fn play_id(hlc: &Hlc) -> String {
    format!("{}.{}.{}", hlc.ms, hlc.n, hlc.device)
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merges an op; what it changed, if anything. Reading an op again changes nothing.
    pub fn apply(&mut self, op: &Op) -> Option<Effect> {
        match &op.change {
            Change::SetField {
                track,
                field,
                values,
            } => self
                .fields
                .entry(track.clone())
                .or_default()
                .entry(field.clone())
                .or_insert_with(Register::new)
                .insert(op.hlc, op.seen.clone(), values.clone())
                .then(|| Effect::Field {
                    track: track.clone(),
                    field: field.clone(),
                    values: values.clone(),
                }),
            Change::SetFavourite { track, on } => self
                .favourites
                .entry(track.clone())
                .or_insert_with(Register::new)
                .insert(op.hlc, op.seen.clone(), *on)
                .then(|| Effect::Favourite {
                    track: track.clone(),
                    on: *on,
                }),
            Change::Trash { track, trashed } => self
                .trash
                .entry(track.clone())
                .or_insert_with(Register::new)
                .insert(op.hlc, op.seen.clone(), *trashed)
                .then(|| Effect::Trash {
                    track: track.clone(),
                    trashed: *trashed,
                }),
            Change::Play {
                track,
                at,
                played_ms,
                completed,
            } => {
                if self.plays.contains_key(&op.hlc) {
                    return None;
                }
                let record = PlayRecord {
                    track: track.clone(),
                    at: *at,
                    played_ms: *played_ms,
                    completed: *completed,
                };
                self.plays.insert(op.hlc, record);
                Some(Effect::Play {
                    track: track.clone(),
                    at: *at,
                    played_ms: *played_ms,
                    completed: *completed,
                    id: play_id(&op.hlc),
                })
            }
            Change::Shared {
                track,
                rel_path,
                size,
            } => {
                let audio = SharedAudio {
                    rel_path: rel_path.clone(),
                    size: *size,
                };
                match self.shared.get(track) {
                    Some((first, _)) if *first <= op.hlc => None,
                    previous => {
                        let fresh = previous.is_none();
                        self.shared.insert(track.clone(), (op.hlc, audio));
                        fresh.then(|| Effect::Shared {
                            track: track.clone(),
                            rel_path: rel_path.clone(),
                            size: *size,
                        })
                    }
                }
            }
        }
    }

    /// The timestamps an edit of this change's register replaces: the current value's, or with
    /// `all`, every version's (resolving a conflict).
    pub fn current(&self, change: &Change, all: bool) -> Vec<Hlc> {
        fn pick<T: Clone + PartialEq>(r: Option<&Register<T>>, all: bool) -> Vec<Hlc> {
            match r {
                Some(r) if all => r.versions.keys().copied().collect(),
                Some(r) => r.winner().map(|w| w.0).into_iter().collect(),
                None => Vec::new(),
            }
        }
        match change {
            Change::SetField { track, field, .. } => {
                pick(self.fields.get(track).and_then(|f| f.get(field)), all)
            }
            Change::SetFavourite { track, .. } => pick(self.favourites.get(track), all),
            Change::Trash { track, .. } => pick(self.trash.get(track), all),
            Change::Play { .. } | Change::Shared { .. } => Vec::new(),
        }
    }

    /// Every value's current version: song, register ("favourite", "trash" or a tag field), the
    /// winning timestamp and the value (a flag as "true"/"false"). A device applies the ones
    /// whose timestamp it hasn't applied yet.
    pub fn winners(&self) -> Vec<(&TrackKey, &str, Hlc, Vec<String>)> {
        let mut out = Vec::new();
        for (track, fields) in &self.fields {
            for (field, r) in fields {
                if let Some((h, v)) = r.winner() {
                    out.push((track, field.as_str(), h, v.clone()));
                }
            }
        }
        for (name, map) in [("favourite", &self.favourites), ("trash", &self.trash)] {
            for (track, r) in map {
                if let Some((h, v)) = r.winner() {
                    out.push((track, name, h, vec![v.to_string()]));
                }
            }
        }
        out
    }

    /// Every synced tag field of a song, with its current values.
    pub fn fields_of(&self, track: &TrackKey) -> Vec<(String, Vec<String>)> {
        self.fields
            .get(track)
            .into_iter()
            .flatten()
            .filter_map(|(f, r)| r.winner().map(|w| (f.clone(), w.1.clone())))
            .collect()
    }

    /// Every synced listen of a song, with its id.
    pub fn plays_of(&self, track: &TrackKey) -> Vec<(String, PlayRecord)> {
        self.plays
            .iter()
            .filter(|(_, p)| p.track == *track)
            .map(|(h, p)| (play_id(h), p.clone()))
            .collect()
    }

    pub fn shared(&self, track: &TrackKey) -> Option<&SharedAudio> {
        self.shared.get(track).map(|s| &s.1)
    }

    /// Every song whose audio is in the store.
    pub fn all_shared(&self) -> impl Iterator<Item = (&TrackKey, &SharedAudio)> {
        self.shared.iter().map(|(k, (_, a))| (k, a))
    }

    /// Every song with anything synced about it.
    pub fn tracks(&self) -> std::collections::BTreeSet<&TrackKey> {
        self.fields
            .keys()
            .chain(self.favourites.keys())
            .chain(self.trash.keys())
            .chain(self.plays.values().map(|p| &p.track))
            .chain(self.shared.keys())
            .collect()
    }

    /// A tag field's synced values, if any device has set it.
    pub fn field(&self, track: &TrackKey, field: &str) -> Option<&[String]> {
        self.fields
            .get(track)?
            .get(field)?
            .winner()
            .map(|w| w.1.as_slice())
    }

    pub fn favourite(&self, track: &TrackKey) -> Option<bool> {
        self.favourites.get(track)?.winner().map(|w| *w.1)
    }

    pub fn trashed(&self, track: &TrackKey) -> Option<bool> {
        self.trash.get(track)?.winner().map(|w| *w.1)
    }

    /// Completed listens of a song on every device.
    pub fn play_count(&self, track: &TrackKey) -> usize {
        self.plays
            .values()
            .filter(|p| p.completed && p.track == *track)
            .count()
    }

    pub fn conflicts(&self) -> Vec<Conflict> {
        let flag = |b: &bool| vec![b.to_string()];
        let mut out = Vec::new();
        for (track, fields) in &self.fields {
            for (field, register) in fields {
                push_lost(&mut out, track, field, register, |v| v.clone());
            }
        }
        for (track, register) in &self.favourites {
            push_lost(&mut out, track, "favourite", register, flag);
        }
        for (track, register) in &self.trash {
            push_lost(&mut out, track, "trash", register, flag);
        }
        out
    }
}

fn push_lost<T: Clone + PartialEq>(
    out: &mut Vec<Conflict>,
    track: &TrackKey,
    field: &str,
    register: &Register<T>,
    text: impl Fn(&T) -> Vec<String>,
) {
    let Some((kept_at, kept)) = register.winner() else {
        return;
    };
    for (lost_at, lost) in register.lost() {
        out.push(Conflict {
            track: track.clone(),
            field: field.to_owned(),
            lost_at,
            lost: text(lost),
            kept_at,
            kept: text(kept),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlc::{Clock, DeviceId};

    fn set(clock: &mut Clock, wall: u64, seen: Option<Hlc>, title: &str) -> Op {
        Op {
            hlc: clock.tick(wall),
            seen: seen.into_iter().collect(),
            change: Change::SetField {
                track: TrackKey::new("h"),
                field: "title".into(),
                values: vec![title.into()],
            },
        }
    }

    #[test]
    fn later_edits_win_and_concurrent_ones_are_kept_as_conflicts() {
        let (mut a, mut b) = (Clock::new(DeviceId::new()), Clock::new(DeviceId::new()));
        let first = set(&mut a, 10, None, "One");
        // B saw "One" and changed it: a plain replacement.
        let second = set(&mut b, 20, Some(first.hlc), "Two");
        // A, not having seen "Two", changes "One" too: concurrent with B's edit.
        let third = set(&mut a, 15, Some(first.hlc), "Three");

        let mut s = State::new();
        for op in [&first, &second, &third] {
            s.apply(op);
        }
        let track = TrackKey::new("h");
        assert_eq!(s.field(&track, "title"), Some(&["Two".to_owned()][..]));
        let conflicts = s.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].lost, ["Three"]);
        assert_eq!(conflicts[0].kept, ["Two"]);

        // The same ops in another order give the same state; reading one again changes nothing.
        let mut t = State::new();
        for op in [&third, &second, &first, &second] {
            t.apply(op);
        }
        assert_eq!(s, t);
    }

    #[test]
    fn plays_add_up_and_effects_report_only_real_changes() {
        let mut c = Clock::new(DeviceId::new());
        let track = TrackKey::new("h");
        let play = |c: &mut Clock, wall| Op {
            hlc: c.tick(wall),
            seen: vec![],
            change: Change::Play {
                track: track.clone(),
                at: 1,
                played_ms: 1000,
                completed: true,
            },
        };
        let mut s = State::new();
        let p1 = play(&mut c, 1);
        assert!(s.apply(&p1).is_some());
        assert!(s.apply(&p1).is_none());
        s.apply(&play(&mut c, 1));
        assert_eq!(s.play_count(&track), 2);

        let newer = set(&mut c, 50, None, "New");
        let older = Op {
            hlc: Hlc { ms: 5, ..newer.hlc },
            ..set(&mut c, 0, None, "Old")
        };
        assert!(s.apply(&newer).is_some());
        assert!(
            s.apply(&older).is_none(),
            "an older edit doesn't change the value"
        );
    }
}
