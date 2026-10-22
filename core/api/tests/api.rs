//! The façade end to end: add a folder, scan in the background, play, count plays, watch.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cadenza_api::{AlbumSort, ApiEvent, Cadenza, PlayerEvent, Status};
use cadenza_engine::Capture;
use cadenza_testkit::fixtures_dir;

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
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

fn wait(what: &str, timeout: Duration, cond: impl Fn() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < timeout, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn add_folder_scan_play_and_watch() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    copy_dir(
        &fixtures_dir().join("library/Alpha Band"),
        &music.join("Alpha Band"),
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let app = Cadenza::open(
        &tmp.path().join("data"),
        Some(Box::new(Capture::new())),
        move |e| sink.lock().unwrap().push(e),
    )
    .unwrap();

    app.add_root(&music).unwrap();
    wait("first scan", Duration::from_secs(20), || {
        events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, ApiEvent::ScanFinished { .. }))
    });
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, ApiEvent::LibraryChanged))
    );
    let albums = app.albums(AlbumSort::Artist, 0, 10).unwrap();
    assert_eq!(albums.len(), 1);
    assert!(app.scan_status().last_report.unwrap().added == 3);

    app.play_album(albums[0].id, 0, false).unwrap();
    wait("album to play through", Duration::from_secs(20), || {
        let finished = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    ApiEvent::Player {
                        event: PlayerEvent::TrackFinished { .. }
                    }
                )
            })
            .count();
        finished == 3 && app.player().status == Status::Stopped
    });
    let tracks = app.album(albums[0].id).unwrap().tracks;
    assert!(tracks.iter().all(|t| t.play_count == 1), "{tracks:?}");

    // A new album copied in shows up on its own (watcher + debounce).
    copy_dir(
        &fixtures_dir().join("library/Gamma/2010 - Night Drive"),
        &music.join("Gamma/Night Drive"),
    );
    wait("watcher rescan", Duration::from_secs(20), || {
        app.albums(AlbumSort::Artist, 0, 10).unwrap().len() == 2
    });
}
