//! Randomised devices making offline edits and syncing in random orders through one store, with
//! clocks that disagree. Once everyone has synced, every device must have the same state, and each
//! device's applied effects must add up to that state.

use std::collections::BTreeMap;

use cadenza_sync::{Change, DeviceId, Effect, MemStore, Replica, State, Store, TrackKey};
use proptest::prelude::*;

const DEVICES: usize = 3;
const TRACKS: u8 = 3;
const FIELDS: [&str; 2] = ["title", "artist"];

#[derive(Debug, Clone)]
enum Action {
    Edit {
        track: u8,
        field: usize,
        value: u8,
    },
    Favourite {
        track: u8,
        on: bool,
    },
    Trash {
        track: u8,
        trashed: bool,
    },
    Play {
        track: u8,
    },
    Share {
        track: u8,
        path: u8,
    },
    /// Resolve the track's title to this value, over every version.
    Resolve {
        track: u8,
        value: u8,
    },
    Push,
    Pull,
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        3 => (0..TRACKS, 0..FIELDS.len(), 0u8..4)
            .prop_map(|(track, field, value)| Action::Edit { track, field, value }),
        1 => (0..TRACKS, any::<bool>()).prop_map(|(track, on)| Action::Favourite { track, on }),
        1 => (0..TRACKS, any::<bool>())
            .prop_map(|(track, trashed)| Action::Trash { track, trashed }),
        1 => (0..TRACKS).prop_map(|track| Action::Play { track }),
        1 => (0..TRACKS, 0u8..3).prop_map(|(track, path)| Action::Share { track, path }),
        1 => (0..TRACKS, 1u8..4).prop_map(|(track, value)| Action::Resolve { track, value }),
        2 => Just(Action::Push),
        2 => Just(Action::Pull),
    ]
}

fn key(track: u8) -> TrackKey {
    TrackKey::new(format!("hash{track}"))
}

/// What a device's library would hold after applying its own changes and the effects it was given.
#[derive(Debug, Default, PartialEq)]
struct View {
    fields: BTreeMap<(TrackKey, String), Vec<String>>,
    favourites: BTreeMap<TrackKey, bool>,
    trash: BTreeMap<TrackKey, bool>,
    plays: BTreeMap<TrackKey, usize>,
    shared: std::collections::BTreeSet<TrackKey>,
}

impl View {
    fn apply(&mut self, effect: Effect) {
        match effect {
            Effect::Field {
                track,
                field,
                values,
            } => {
                self.fields.insert((track, field), values);
            }
            Effect::Favourite { track, on } => {
                self.favourites.insert(track, on);
            }
            Effect::Trash { track, trashed } => {
                self.trash.insert(track, trashed);
            }
            Effect::Play {
                track, completed, ..
            } => {
                if completed {
                    *self.plays.entry(track).or_default() += 1;
                }
            }
            Effect::Shared { track, .. } => {
                assert!(self.shared.insert(track), "a song is shared once");
            }
        }
    }

    fn matches(&self, state: &State) -> bool {
        (0..TRACKS).all(|t| {
            let k = key(t);
            FIELDS.iter().all(|f| {
                state.field(&k, f)
                    == self
                        .fields
                        .get(&(k.clone(), (*f).to_owned()))
                        .map(|v| &v[..])
            }) && state.favourite(&k) == self.favourites.get(&k).copied()
                && state.trashed(&k) == self.trash.get(&k).copied()
                && state.play_count(&k) == self.plays.get(&k).copied().unwrap_or(0)
                && state.shared(&k).is_some() == self.shared.contains(&k)
        })
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn devices_converge(
        steps in prop::collection::vec((0..DEVICES, action()), 1..80),
        skews in prop::array::uniform3(-5_000i64..5_000),
    ) {
        let store = MemStore::new();
        let mut replicas: Vec<Replica> = (0..DEVICES).map(|_| Replica::new(DeviceId::new())).collect();
        let mut views: Vec<View> = (0..DEVICES).map(|_| View::default()).collect();
        let mut wall = 1_700_000_000_000i64;
        for (d, action) in steps {
            wall += 7;
            let now = (wall + skews[d]) as u64;
            let (replica, view) = (&mut replicas[d], &mut views[d]);
            let change = match action {
                Action::Edit { track, field, value } => Some(Change::SetField {
                    track: key(track),
                    field: FIELDS[field].into(),
                    values: if value == 0 { vec![] } else { vec![format!("v{value}")] },
                }),
                Action::Favourite { track, on } => Some(Change::SetFavourite { track: key(track), on }),
                Action::Trash { track, trashed } => Some(Change::Trash { track: key(track), trashed }),
                Action::Play { track } => Some(Change::Play {
                    track: key(track),
                    at: wall / 1000,
                    played_ms: 1000,
                    completed: true,
                }),
                Action::Share { track, path } => Some(Change::Shared {
                    track: key(track),
                    rel_path: format!("Artist/Album/{path}.flac"),
                    size: 100,
                }),
                Action::Resolve { track, value } => {
                    let op = replica.resolve(
                        Change::SetField {
                            track: key(track),
                            field: "title".into(),
                            values: vec![format!("v{value}")],
                        },
                        now,
                    );
                    let mut s = State::new();
                    if let Some(e) = s.apply(&op) {
                        view.apply(e);
                    }
                    let k = key(track);
                    prop_assert!(
                        !replica.state().conflicts().iter().any(|c| c.track == k && c.field == "title"),
                        "resolving clears the conflict here"
                    );
                    None
                }
                Action::Push => {
                    replica.push(&store, None, now).expect("push");
                    None
                }
                Action::Pull => {
                    replica.pull(&store, |e| view.apply(e)).expect("pull");
                    None
                }
            };
            if let Some(change) = change {
                let op = replica.record(change, now);
                // A local change applies to the library directly, like an edit in Song Info. A
                // device sharing a song already has it.
                let mut s = State::new();
                match s.apply(&op) {
                    Some(Effect::Shared { track, .. }) => {
                        view.shared.insert(track);
                    }
                    Some(e) => view.apply(e),
                    None => {}
                }
            }
        }
        for _ in 0..2 {
            for (r, v) in replicas.iter_mut().zip(views.iter_mut()) {
                r.sync(&store, Some(("test", "linux")), 0, |e| v.apply(e)).expect("sync");
            }
        }
        let first = replicas[0].state();
        for (r, v) in replicas.iter().zip(&views) {
            prop_assert_eq!(r.state(), first);
            prop_assert_eq!(r.state().conflicts(), first.conflicts());
            prop_assert!(v.matches(r.state()), "view {:?}", v);
        }
    }
}

#[test]
fn a_replica_survives_a_restart_and_resumes_where_it_was() {
    let store = MemStore::new();
    let mut a = Replica::new(DeviceId::new());
    let mut b = Replica::new(DeviceId::new());
    let track = key(0);
    a.record(
        Change::SetFavourite {
            track: track.clone(),
            on: true,
        },
        10,
    );
    a.push(&store, Some(("Laptop", "linux")), 10).expect("push");
    b.pull(&store, |_| ()).expect("pull");

    let mut b = Replica::from_bytes(&b.to_bytes().expect("save")).expect("load");
    assert_eq!(b.state().favourite(&track), Some(true));
    let mut effects = 0;
    b.pull(&store, |_| effects += 1).expect("pull");
    assert_eq!(effects, 0, "nothing is read twice after a restart");
    assert!(
        store
            .get(&format!("devices/{}.json", a.device()))
            .expect("get")
            .is_some()
    );
}

#[test]
fn a_gap_in_a_log_waits_for_the_missing_segment() {
    let full = MemStore::new();
    let mut a = Replica::new(DeviceId::new());
    for i in 0..3 {
        a.record(
            Change::Play {
                track: key(0),
                at: i,
                played_ms: 1,
                completed: true,
            },
            i as u64,
        );
        a.push(&full, None, 0).expect("push");
    }
    // A copy of the store that has segments 0 and 2 but not yet 1.
    let partial = MemStore::new();
    for seq in [0, 2] {
        let path = format!("log/{}/{seq:016}.seg", a.device());
        partial
            .put(&path, &full.get(&path).expect("get").expect("there"))
            .expect("put");
    }
    let mut b = Replica::new(DeviceId::new());
    b.pull(&partial, |_| ()).expect("pull");
    assert_eq!(b.state().play_count(&key(0)), 1);
    b.pull(&full, |_| ()).expect("pull");
    assert_eq!(b.state().play_count(&key(0)), 3);
}
