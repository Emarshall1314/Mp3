//! Sync end to end: separate Cadenza libraries on "devices" paired over the network (here, this
//! machine's loopback). Edits, favourites, listens, conflicts, the trash, artwork and songs reach
//! the other devices' files and catalogues.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cadenza_api::{ApiEvent, Cadenza, PictureEdit, PlayerEvent, SongSort, SyncSettings, TrackEdit};
use cadenza_engine::Capture;
use cadenza_testkit::fixtures_dir;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dst);
        } else {
            std::fs::copy(e.path(), &dst).unwrap();
        }
    }
}

/// Devices find each other by address here, not by mDNS, so tests don't depend on the network
/// (or on each other's devices).
fn no_discovery() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: set once, before any test opens a library; nothing else sets the environment.
    ONCE.call_once(|| unsafe { std::env::set_var("CADENZA_NO_DISCOVERY", "1") });
}

fn wait(what: &str, cond: impl Fn() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Device {
    app: Cadenza,
    events: Arc<Mutex<Vec<ApiEvent>>>,
    music: PathBuf,
}

impl Device {
    fn open(base: &Path, name: &str, with_music: bool) -> Self {
        no_discovery();
        let music = base.join(name).join("music");
        std::fs::create_dir_all(&music).unwrap();
        if with_music {
            copy_dir(
                &fixtures_dir().join("library/Alpha Band"),
                &music.join("Alpha Band"),
            );
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let app = Cadenza::open(
            &base.join(name).join("data"),
            Some(Box::new(Capture::new())),
            move |e| sink.lock().unwrap().push(e),
        )
        .unwrap();
        let d = Self { app, events, music };
        d.app.add_root(&d.music).unwrap();
        d.wait_scans(1);
        d.settings(|s| s.device_name = name.into());
        d
    }

    fn settings(&self, f: impl FnOnce(&mut SyncSettings)) {
        let mut s = self.app.sync_status().settings;
        f(&mut s);
        self.app.set_sync_settings(s).unwrap();
    }

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.app.sync_status().port)
    }

    /// Joins `other`'s library (starting one there if it has none).
    fn join(&self, other: &Device) {
        let status = other.app.sync_status();
        let code = match status.pairing_code {
            Some(c) => c,
            None => other.app.sync_start().unwrap().pairing_code.unwrap(),
        };
        self.app.sync_join(&other.address(), &code).unwrap();
    }

    fn count(&self, f: impl Fn(&ApiEvent) -> bool) -> usize {
        self.events.lock().unwrap().iter().filter(|e| f(e)).count()
    }

    fn scans(&self) -> usize {
        self.count(|e| matches!(e, ApiEvent::ScanFinished { .. }))
    }

    fn edits(&self) -> usize {
        self.count(|e| matches!(e, ApiEvent::EditFinished { .. }))
    }

    fn wait_scans(&self, n: usize) {
        wait("a scan", || {
            self.scans() >= n && !self.app.scan_status().running
        });
    }

    /// Syncs; if that starts tag writes, waits for them and the scan after.
    fn sync(&self) {
        let edits = self.edits();
        let status = self.app.sync_blocking();
        assert_eq!(status.error, None, "{status:?}");
        std::thread::sleep(Duration::from_millis(50));
        if self.app.edit_status().running || self.edits() > edits {
            wait("synced tag changes", || self.edits() > edits);
            wait("the scan after", || !self.app.scan_status().running);
        }
    }

    fn song(&self, title: &str) -> i64 {
        self.app
            .songs(SongSort::Title, false, 0, 1000)
            .unwrap()
            .into_iter()
            .find(|t| t.title == title)
            .unwrap_or_else(|| panic!("no song {title}"))
            .id
    }

    fn songs(&self) -> Vec<String> {
        self.app
            .songs(SongSort::Title, false, 0, 1000)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect()
    }

    fn field(&self, id: i64, field: &str) -> Vec<String> {
        self.app.track_tags(&[id]).unwrap()[0]
            .fields
            .get(field)
            .cloned()
            .unwrap_or_default()
    }

    fn edit(&self, id: i64, field: &str, value: &str) {
        let n = self.edits();
        self.app
            .edit_tracks(
                vec![TrackEdit {
                    track_id: id,
                    fields: BTreeMap::from([(field.to_owned(), vec![value.to_owned()])]),
                    pictures: None,
                }],
                "test edit",
            )
            .unwrap();
        wait("an edit", || self.edits() > n);
        wait("the scan after", || !self.app.scan_status().running);
    }
}

#[test]
fn two_devices_share_edits_favourites_listens_conflicts_and_the_trash() {
    let tmp = tempfile::tempdir().unwrap();
    let a = Device::open(tmp.path(), "laptop", true);
    let b = Device::open(tmp.path(), "desktop", true);
    b.join(&a);
    let titles = a.songs();
    let (first, second, third) = (&titles[0], &titles[1], &titles[2]);

    // An edit, a favourite and a listen on A reach B.
    let x = a.song(first);
    a.edit(x, "title", "Renamed on the laptop");
    let y = a.song(second);
    a.app.set_favourite(&[y], true).unwrap();
    let plays = a.count(|e| {
        matches!(
            e,
            ApiEvent::Player {
                event: PlayerEvent::TrackFinished { .. }
            }
        )
    });
    a.app.play_tracks(&[y], 0, false).unwrap();
    wait("a listen", || {
        a.app.tracks(&[y]).unwrap()[0].play_count == 1
            && a.count(|e| {
                matches!(
                    e,
                    ApiEvent::Player {
                        event: PlayerEvent::TrackFinished { .. }
                    }
                )
            }) > plays
    });
    a.sync();
    b.sync();
    let bx = b.song("Renamed on the laptop");
    let by = b.song(second);
    assert_eq!(b.field(bx, "title"), ["Renamed on the laptop"]);
    let row = &b.app.tracks(&[by]).unwrap()[0];
    assert!(row.favourite);
    assert_eq!(row.play_count, 1);
    // Syncing again changes nothing and counts nothing twice.
    b.sync();
    a.sync();
    assert_eq!(b.app.tracks(&[by]).unwrap()[0].play_count, 1);
    assert_eq!(a.app.tracks(&[y]).unwrap()[0].play_count, 1);
    let devices = a.app.sync_status().devices;
    assert_eq!(devices.len(), 2, "{devices:?}");
    assert!(devices[0].this_device && devices[0].name == "laptop");

    // Concurrent edits: one wins everywhere, the other is a conflict on both devices until
    // resolved.
    a.edit(x, "artist", "Laptop Artist");
    b.edit(bx, "artist", "Desktop Artist");
    a.sync();
    b.sync();
    a.sync();
    let ca = a.app.sync_conflicts();
    let cb = b.app.sync_conflicts();
    assert_eq!(ca.len(), 1, "{ca:?}");
    assert_eq!(ca, cb);
    let winner = ca[0].kept.clone();
    assert_eq!(a.field(x, "artist"), winner);
    assert_eq!(b.field(bx, "artist"), winner);
    assert_eq!(ca[0].title.as_deref(), Some("Renamed on the laptop"));
    a.app
        .resolve_conflict(&ca[0].audio_hash, "artist", ca[0].lost.clone())
        .unwrap();
    a.sync();
    b.sync();
    a.sync();
    assert!(a.app.sync_conflicts().is_empty());
    assert!(b.app.sync_conflicts().is_empty());
    assert_eq!(a.field(x, "artist"), ca[0].lost);
    assert_eq!(b.field(bx, "artist"), ca[0].lost);

    // The trash: trashing on A trashes B's copy; restoring on A restores it.
    let z = a.song(third);
    let bz = b.song(third);
    let bz_path = b.app.track_tags(&[bz]).unwrap()[0].path.clone();
    a.app.trash(&[z]).unwrap();
    a.sync();
    b.sync();
    assert!(!Path::new(&bz_path).exists());
    assert_eq!(b.app.trash_items().unwrap().len(), 1);
    assert!(!b.songs().contains(third));
    let item = a.app.trash_items().unwrap()[0].id;
    a.app.restore(item).unwrap();
    a.sync();
    let scans = b.scans();
    b.sync();
    wait("B's rescan", || {
        b.scans() > scans && !b.app.scan_status().running
    });
    assert!(Path::new(&bz_path).exists());
    assert!(b.app.trash_items().unwrap().is_empty());
    assert!(b.songs().contains(third));
    assert_eq!(
        b.app.tracks(&[bz]).unwrap()[0].id,
        bz,
        "the same song came back"
    );
}

#[test]
fn a_new_device_gets_shared_songs_with_their_history() {
    let tmp = tempfile::tempdir().unwrap();
    let a = Device::open(tmp.path(), "laptop", true);
    let titles = a.songs();
    let x = a.song(&titles[0]);
    a.app.set_favourite(&[x], true).unwrap();
    a.settings(|s| s.share_audio = true);
    a.app.sync_start().unwrap();
    a.sync();

    let phone = Device::open(tmp.path(), "phone", false);
    assert!(phone.songs().is_empty());
    phone.settings(|s| s.download_audio = true);
    phone.join(&a);
    let scans = phone.scans();
    phone.sync();
    wait("the scan of downloaded songs", || {
        phone.scans() > scans && !phone.app.scan_status().running
    });
    // Favourites apply to songs once they're here.
    phone.sync();
    let mut got = phone.songs();
    let mut want = titles.clone();
    got.sort();
    want.sort();
    assert_eq!(got, want);
    let px = phone.song(&titles[0]);
    assert!(phone.app.tracks(&[px]).unwrap()[0].favourite);
    let path = phone.app.track_tags(&[px]).unwrap()[0].path.clone();
    assert!(path.starts_with(&phone.music.display().to_string()));
    // Nothing half-downloaded is left behind.
    let mut stack = vec![phone.music.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            assert!(!p.to_string_lossy().ends_with(".part"), "{}", p.display());
            if p.is_dir() {
                stack.push(p);
            }
        }
    }
}

#[test]
fn only_devices_with_the_pairing_code_join() {
    let tmp = tempfile::tempdir().unwrap();
    let a = Device::open(tmp.path(), "laptop", true);
    let b = Device::open(tmp.path(), "desktop", true);
    // Nothing to join until A starts syncing.
    assert!(b.app.sync_join(&a.address(), "ABCD-EFGH-JK23").is_err());
    let code = a.app.sync_start().unwrap().pairing_code.unwrap();
    assert!(a.app.sync_start().is_err(), "already syncing");
    assert!(b.app.sync_join(&a.address(), "ABCD-EFGH-JK23").is_err());
    assert!(b.app.sync_join(&a.address(), "not a code").is_err());
    assert!(!b.app.sync_status().paired);
    // Typed any old way, the right code works; both show it.
    let typed = code.to_lowercase().replace('-', " ");
    let status = b.app.sync_join(&a.address(), &typed).unwrap();
    assert!(status.paired);
    assert_eq!(status.pairing_code.as_deref(), Some(code.as_str()));
    assert_eq!(
        status.settings.addresses,
        [a.address()],
        "a typed address is kept for later rounds"
    );
    b.sync();
    a.sync();
    let devices = a.app.sync_status().devices;
    assert_eq!(devices.len(), 2, "{devices:?}");
    assert!(devices.iter().any(|d| d.name == "desktop" && d.online));

    // A device in another library gets nothing from A, and A nothing from it.
    let c = Device::open(tmp.path(), "stranger", true);
    c.app.sync_start().unwrap();
    c.settings(|s| s.addresses = vec![a.address()]);
    c.sync();
    assert_eq!(c.app.sync_status().devices.len(), 1);

    // Leaving: B stops syncing and forgets the library; its music stays.
    let songs = b.songs();
    let status = b.app.sync_leave().unwrap();
    assert!(!status.paired && status.pairing_code.is_none());
    assert_eq!(b.songs(), songs);
}

#[test]
fn new_artwork_travels_to_the_other_device() {
    let tmp = tempfile::tempdir().unwrap();
    let a = Device::open(tmp.path(), "laptop", true);
    let b = Device::open(tmp.path(), "desktop", true);
    b.join(&a);
    let title = a.songs()[0].clone();
    let x = a.song(&title);

    // An image only A has.
    let art = a
        .app
        .import_artwork(&fixtures_dir().join("artwork/folder_art_album/back.jpg"))
        .unwrap();
    let n = a.edits();
    a.app
        .edit_tracks(
            vec![TrackEdit {
                track_id: x,
                fields: BTreeMap::new(),
                pictures: Some(PictureEdit::SetFront {
                    hash: art.hash.clone(),
                }),
            }],
            "new cover",
        )
        .unwrap();
    wait("the cover edit", || a.edits() > n);
    wait("the scan after", || !a.app.scan_status().running);
    a.sync();
    b.sync();
    let bx = b.song(&title);
    let front = b.app.track_tags(&[bx]).unwrap()[0]
        .pictures
        .iter()
        .find(|p| p.picture_type == 3)
        .and_then(|p| p.hash.clone());
    assert_eq!(front.as_deref(), Some(art.hash.as_str()));
}

#[test]
fn another_device_carries_on_where_this_one_stopped() {
    let tmp = tempfile::tempdir().unwrap();
    let a = Device::open(tmp.path(), "laptop", true);
    let b = Device::open(tmp.path(), "desktop", true);
    b.join(&a);
    let titles = a.songs();
    let ids: Vec<i64> = titles.iter().map(|t| a.song(t)).collect();
    // The test output plays faster than real time and the songs are short: pause at once.
    a.app.play_tracks(&ids, 1, false).unwrap();
    a.app.pause();
    wait("a queue", || {
        let s = a.app.player();
        s.queue_length == ids.len()
            && s.track_id.is_some()
            && s.status != cadenza_api::Status::Playing
    });
    let at = a.app.player().index;
    a.sync();
    b.sync();

    let offers = b.app.sync_status().handoffs;
    assert_eq!(offers.len(), 1, "{offers:?}");
    assert_eq!(offers[0].device_name, "laptop");
    assert_eq!(offers[0].songs, ids.len());
    assert_eq!(offers[0].here, ids.len());
    let playing_title = a.app.tracks(&[a.app.player().track_id.unwrap()]).unwrap()[0]
        .title
        .clone();
    assert_eq!(offers[0].title, playing_title);
    assert!(
        a.app.sync_status().handoffs.is_empty(),
        "a device isn't offered its own"
    );

    b.app.continue_from(&offers[0].device_id).unwrap();
    b.app.pause();
    // The test output plays faster than real time, so under load B may already have moved past
    // the song before the pause lands. What must hold: the same queue, in the same order, with
    // the laptop's song at the place it stopped, and B starting there (not before).
    wait("carrying on", || b.app.player().queue_length == ids.len());
    let queue: Vec<String> = b
        .app
        .queue_window(0, ids.len())
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    let expected: Vec<String> = a
        .app
        .queue_window(0, ids.len())
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(queue, expected);
    let at = at.expect("the laptop stopped in its queue");
    assert_eq!(queue[at], playing_title);
    // Still at that song or past it (or finished the queue), never before it.
    assert!(b.app.player().index.is_none_or(|i| i >= at));
}
