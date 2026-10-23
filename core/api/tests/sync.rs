//! Sync end to end: separate Cadenza libraries on "devices" sharing one folder. Edits, favourites,
//! listens, conflicts, the trash and shared audio reach the other devices' files and catalogues.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cadenza_api::{
    ApiEvent, Cadenza, PictureEdit, PlayerEvent, Remote, SongSort, SyncSettings, TrackEdit,
};
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
    fn open(base: &Path, name: &str, with_music: bool, folder: &Path) -> Self {
        Self::open_with(base, name, with_music, folder, None)
    }

    fn open_with(
        base: &Path,
        name: &str,
        with_music: bool,
        folder: &Path,
        passphrase: Option<&str>,
    ) -> Self {
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
        d.app
            .set_sync_settings(
                SyncSettings {
                    folder: Some(folder.display().to_string()),
                    device_name: name.into(),
                    ..Default::default()
                },
                passphrase,
                None,
            )
            .unwrap();
        d
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
    let folder = tmp.path().join("shared");
    std::fs::create_dir_all(&folder).unwrap();
    let a = Device::open(tmp.path(), "laptop", true, &folder);
    let b = Device::open(tmp.path(), "desktop", true, &folder);
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
    let folder = tmp.path().join("shared");
    std::fs::create_dir_all(&folder).unwrap();
    let a = Device::open(tmp.path(), "laptop", true, &folder);
    let titles = a.songs();
    let x = a.song(&titles[0]);
    a.app.set_favourite(&[x], true).unwrap();
    a.app
        .set_sync_settings(
            SyncSettings {
                folder: Some(folder.display().to_string()),
                device_name: "laptop".into(),
                share_audio: true,
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
    a.sync();
    assert!(folder.join("blobs").is_dir());

    let phone = Device::open(tmp.path(), "phone", false, &folder);
    assert!(phone.songs().is_empty());
    phone
        .app
        .set_sync_settings(
            SyncSettings {
                folder: Some(folder.display().to_string()),
                device_name: "phone".into(),
                download_audio: true,
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
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
fn the_sync_folder_must_be_outside_the_music_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("shared");
    std::fs::create_dir_all(&folder).unwrap();
    let a = Device::open(tmp.path(), "laptop", true, &folder);
    let inside = a.music.join("sync");
    std::fs::create_dir_all(&inside).unwrap();
    assert!(
        a.app
            .set_sync_settings(
                SyncSettings {
                    folder: Some(inside.display().to_string()),
                    ..Default::default()
                },
                None,
                None
            )
            .is_err()
    );
    assert!(
        a.app
            .set_sync_settings(
                SyncSettings {
                    folder: Some(tmp.path().join("nowhere").display().to_string()),
                    ..Default::default()
                },
                None,
                None
            )
            .is_err()
    );
}

#[test]
fn new_artwork_travels_through_the_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("shared");
    std::fs::create_dir_all(&folder).unwrap();
    let a = Device::open(tmp.path(), "laptop", true, &folder);
    let b = Device::open(tmp.path(), "desktop", true, &folder);
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
    assert!(
        folder.join("art").is_dir(),
        "the image went into the folder"
    );
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
fn an_encrypted_folder_holds_nothing_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("shared");
    let other = tmp.path().join("other");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let pass = "correct horse battery staple";
    let a = Device::open_with(tmp.path(), "laptop", true, &folder, Some(pass));
    assert!(a.app.sync_status().encrypted);
    let title = a.songs()[0].clone();
    let x = a.song(&title);
    a.edit(x, "title", "A Secret Title");
    a.app
        .set_sync_settings(
            SyncSettings {
                folder: Some(folder.display().to_string()),
                device_name: "laptop".into(),
                share_audio: true,
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
    a.sync();

    let mut files = Vec::new();
    let mut stack = vec![folder.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                files.push(p);
            }
        }
    }
    assert!(files.iter().any(|f| f.starts_with(folder.join("blobs"))));
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy();
        assert!(!name.ends_with(".flac"), "{name}");
        let bytes = std::fs::read(f).unwrap();
        for secret in [&b"A Secret Title"[..], b"laptop", b"fLaC"] {
            assert!(
                !bytes.windows(secret.len()).any(|w| w == secret),
                "{} holds {:?}",
                f.display(),
                String::from_utf8_lossy(secret)
            );
        }
    }
    let info = a
        .app
        .sync_folder_info(&folder.display().to_string())
        .unwrap();
    assert!(info.has_library && info.encrypted);

    // Joining needs the passphrase, and the right one.
    let b = Device::open(tmp.path(), "desktop", true, &other);
    let join = |p: Option<&str>| {
        b.app.set_sync_settings(
            SyncSettings {
                folder: Some(folder.display().to_string()),
                device_name: "desktop".into(),
                ..Default::default()
            },
            p,
            None,
        )
    };
    assert!(join(None).is_err());
    assert!(join(Some("wrong horse battery staple")).is_err());
    join(Some(pass)).unwrap();
    b.sync();
    assert!(b.songs().contains(&"A Secret Title".to_owned()));
}

/// Through a WebDAV server, encrypted (needs `CADENZA_TEST_WEBDAV`; `cargo xtask` starts one).
#[test]
fn two_devices_sync_through_a_webdav_server() {
    let Ok(spec) = std::env::var("CADENZA_TEST_WEBDAV") else {
        eprintln!("skipped: no WebDAV server given");
        return;
    };
    let mut parts = spec.split('|');
    let (url, user, pass) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    let remote = Remote::WebDav {
        url: format!("{url}/api-{}", std::process::id()),
        username: user.into(),
    };
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("unused");
    std::fs::create_dir_all(&local).unwrap();
    let a = Device::open(tmp.path(), "laptop", true, &local);
    let b = Device::open(tmp.path(), "desktop", true, &local);
    let phrase = "correct horse battery staple";
    let use_server = |d: &Device, name: &str, passphrase: Option<&str>, password: Option<&str>| {
        d.app.set_sync_settings(
            SyncSettings {
                remote: Some(remote.clone()),
                device_name: name.into(),
                ..Default::default()
            },
            passphrase,
            password,
        )
    };
    // A wrong password is refused before anything is saved.
    assert!(use_server(&a, "laptop", Some(phrase), Some("not the password")).is_err());
    let info = a.app.sync_remote_info(&remote, pass).unwrap();
    assert!(!info.has_library);
    use_server(&a, "laptop", Some(phrase), Some(pass)).unwrap();
    assert!(a.app.sync_status().encrypted);
    let title = a.songs()[0].clone();
    let x = a.song(&title);
    a.edit(x, "title", "Sent Over WebDAV");
    a.sync();

    let info = b.app.sync_remote_info(&remote, pass).unwrap();
    assert!(info.has_library && info.encrypted && info.devices == 1);
    assert!(
        use_server(&b, "desktop", None, Some(pass)).is_err(),
        "needs the passphrase"
    );
    use_server(&b, "desktop", Some(phrase), Some(pass)).unwrap();
    b.sync();
    assert!(b.songs().contains(&"Sent Over WebDAV".to_owned()));
    // The password isn't in the settings the UI sees.
    let shown = serde_json::to_string(&b.app.sync_status()).unwrap();
    assert!(!shown.contains(pass));
}

#[test]
fn another_device_carries_on_where_this_one_stopped() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("shared");
    std::fs::create_dir_all(&folder).unwrap();
    let a = Device::open(tmp.path(), "laptop", true, &folder);
    let b = Device::open(tmp.path(), "desktop", true, &folder);
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
