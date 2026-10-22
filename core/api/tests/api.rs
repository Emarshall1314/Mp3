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

#[test]
fn replay_gain_measures_untagged_albums_and_plays_at_album_gain() {
    use cadenza_api::{ReplayGainMode, ReplayGainSettings};

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
        Some(Box::new(Capture::paced(20.0))),
        move |e| sink.lock().unwrap().push(e),
    )
    .unwrap();
    app.add_root(&music).unwrap();
    wait("first scan", Duration::from_secs(20), || {
        app.scan_status().last_report.is_some()
    });
    app.set_replay_gain(ReplayGainSettings {
        mode: ReplayGainMode::Auto,
        ..ReplayGainSettings::default()
    })
    .unwrap();
    wait("loudness measured", Duration::from_secs(30), || {
        let s = app.loudness_status();
        !s.running && s.total == 3 && s.done == 3
    });

    let album = app.albums(AlbumSort::Artist, 0, 1).unwrap()[0].id;
    app.play_album(album, 0, false).unwrap();
    wait("album to play through", Duration::from_secs(20), || {
        app.player().status == Status::Stopped
            && events.lock().unwrap().iter().any(|e| {
                matches!(
                    e,
                    ApiEvent::Player {
                        event: PlayerEvent::TrackFinished { .. }
                    }
                )
            })
    });
    let mut gains = Vec::new();
    for e in events.lock().unwrap().iter() {
        if let ApiEvent::Player {
            event: PlayerEvent::State(s),
        } = e
            && let (Some(id), Some(info)) = (s.track_id, &s.stream)
            && gains.last().is_none_or(|(last, _)| *last != id)
        {
            gains.push((id, info.replay_gain_db.unwrap()));
        }
    }
    assert_eq!(gains.len(), 3, "{gains:?}");
    // One album gain for every track, a plausible value for test tones.
    assert!(gains.iter().all(|(_, g)| *g == gains[0].1), "{gains:?}");
    assert!((-20.0..20.0).contains(&gains[0].1), "{gains:?}");
}

#[test]
fn appearance_is_saved_across_restarts() {
    use cadenza_api::{Appearance, Theme};

    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let open = || Cadenza::open(&data, Some(Box::new(Capture::new())), |_| {}).unwrap();
    let app = open();
    assert_eq!(app.settings().appearance, Appearance::default());
    let chosen = Appearance {
        theme: Theme::Dark,
        reduce_motion: true,
        reduce_transparency: false,
    };
    app.set_appearance(chosen).unwrap();
    assert_eq!(app.settings().appearance, chosen);
    drop(app);
    assert_eq!(open().settings().appearance, chosen);
}

#[test]
fn tag_edits_reach_files_and_library_and_can_be_undone() {
    use cadenza_api::TrackEdit;

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
    let scans = || {
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, ApiEvent::ScanFinished { .. }))
            .count()
    };
    let finished = || -> Vec<cadenza_api::EditReport> {
        events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                ApiEvent::EditFinished { report } => Some(report.clone()),
                _ => None,
            })
            .collect()
    };
    wait("first scan", Duration::from_secs(20), || scans() >= 1);

    let track = app
        .songs(cadenza_api::SongSort::Title, false, 0, 1)
        .unwrap()[0]
        .clone();
    let original = track.title.clone();
    let tags = app.track_tags(&[track.id]).unwrap();
    assert_eq!(tags[0].fields["title"], vec![original.clone()]);
    assert!(tags[0].read_only.is_none());

    let edit = |title: &str| TrackEdit {
        track_id: track.id,
        fields: [("title".to_owned(), vec![title.to_owned()])].into(),
        pictures: None,
    };
    let scans_before = scans();
    let batch = app.edit_tracks(vec![edit("Renamed ✓")], "Rename").unwrap();
    wait("edit", Duration::from_secs(20), || finished().len() == 1);
    let report = &finished()[0];
    assert_eq!(
        (report.batch, report.written, report.failed.len()),
        (batch, 1, 0)
    );
    wait("rescan", Duration::from_secs(20), || scans() > scans_before);
    let now = &app.tracks(&[track.id]).unwrap()[0];
    assert_eq!(now.title, "Renamed ✓", "same track id, new title");

    let history = app.edit_history(10).unwrap();
    assert_eq!(history[0].description, "Rename");
    assert_eq!(history[0].tracks, 1);
    let changes = app.edit_changes(batch).unwrap();
    assert_eq!(changes[0].old, vec![original.clone()]);
    assert_eq!(changes[0].new, vec!["Renamed ✓".to_owned()]);

    let scans_before = scans();
    app.undo_edit(batch).unwrap();
    wait("undo", Duration::from_secs(20), || finished().len() == 2);
    assert_eq!(finished()[1].written, 1);
    wait("rescan", Duration::from_secs(20), || scans() > scans_before);
    assert_eq!(app.tracks(&[track.id]).unwrap()[0].title, original);
    let history = app.edit_history(10).unwrap();
    assert!(history[1].undone, "the first batch is marked undone");
    assert!(app.undo_edit(batch).is_err(), "can't undo twice");

    // An undo leaves alone fields changed since the edit.
    let batch = app.edit_tracks(vec![edit("Second")], "Again").unwrap();
    wait("edit", Duration::from_secs(20), || finished().len() == 3);
    let path = std::path::PathBuf::from(&app.track_tags(&[track.id]).unwrap()[0].path);
    let mut elsewhere = cadenza_tags::Edit::default();
    elsewhere
        .fields
        .insert("title".into(), vec!["Changed elsewhere".into()]);
    cadenza_tags::write(&path, &elsewhere, &Default::default()).unwrap();
    app.undo_edit(batch).unwrap();
    wait("undo", Duration::from_secs(20), || finished().len() == 4);
    let report = &finished()[3];
    assert_eq!(report.written, 0);
    assert!(
        report.failed[0].error.contains("changed since"),
        "{:?}",
        report.failed
    );
    assert_eq!(
        cadenza_tags::read(&path).unwrap().get("title"),
        Some("Changed elsewhere")
    );
}

#[test]
fn artwork_edits_replace_the_cover_and_can_be_undone() {
    use cadenza_api::{PictureEdit, TrackEdit};

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
        app.scan_status().last_report.is_some()
    });
    let finished = || {
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, ApiEvent::EditFinished { .. }))
            .count()
    };
    let track = app
        .songs(cadenza_api::SongSort::Title, false, 0, 1)
        .unwrap()[0]
        .clone();
    let before = app.track_tags(&[track.id]).unwrap()[0].pictures.clone();

    // A new image from another fixture, through a file as a user would pick it.
    let other = cadenza_testkit::manifest()
        .entries
        .iter()
        .find(|e| e.id == "artwork/flac_many_types")
        .unwrap();
    let picture = cadenza_tags::read(&other.file())
        .unwrap()
        .pictures
        .remove(0);
    let image = tmp.path().join("new-cover.img");
    std::fs::write(&image, &picture.data).unwrap();
    let stored = app.import_artwork(&image).unwrap();
    assert!(
        app.import_artwork(&tmp.path().join("music/Alpha Band"))
            .is_err()
    );

    let batch = app
        .edit_tracks(
            vec![TrackEdit {
                track_id: track.id,
                fields: Default::default(),
                pictures: Some(PictureEdit::SetFront {
                    hash: stored.hash.clone(),
                }),
            }],
            "New cover",
        )
        .unwrap();
    wait("cover edit", Duration::from_secs(20), || finished() == 1);
    let now = app.track_tags(&[track.id]).unwrap()[0].pictures.clone();
    assert_eq!(now[0].picture_type, 3);
    assert_eq!(now[0].hash.as_deref(), Some(stored.hash.as_str()));
    assert_eq!(app.edit_changes(batch).unwrap()[0].field, "pictures");

    app.undo_edit(batch).unwrap();
    wait("undo", Duration::from_secs(20), || finished() == 2);
    let restored = app.track_tags(&[track.id]).unwrap()[0].pictures.clone();
    assert_eq!(restored, before, "the original artwork is back");

    let cover = app.save_cover_to_folder(track.id);
    if before.is_empty() {
        assert!(cover.is_err());
    } else {
        let path = cover.unwrap();
        assert!(path.exists());
        assert!(
            app.save_cover_to_folder(track.id).is_err(),
            "never overwrites"
        );
    }
}

#[test]
fn the_organiser_moves_files_keeps_their_history_and_can_undo() {
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
    let scans = || {
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, ApiEvent::ScanFinished { .. }))
            .count()
    };
    let finished = || {
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, ApiEvent::EditFinished { .. }))
            .count()
    };
    wait("first scan", Duration::from_secs(20), || scans() >= 1);
    let album = app.albums(AlbumSort::Artist, 0, 1).unwrap()[0].clone();
    let ids: Vec<i64> = app
        .album(album.id)
        .unwrap()
        .tracks
        .iter()
        .map(|t| t.id)
        .collect();
    app.play_tracks(&ids[..1], 0, false).unwrap();
    wait("a play", Duration::from_secs(20), || {
        events.lock().unwrap().iter().any(|e| {
            matches!(
                e,
                ApiEvent::Player {
                    event: PlayerEvent::TrackFinished { .. }
                }
            )
        }) && app.tracks(&ids[..1]).unwrap()[0].play_count == 1
    });
    let before: Vec<String> = app
        .track_tags(&ids)
        .unwrap()
        .into_iter()
        .map(|t| t.path)
        .collect();

    let pattern = "Sorted/%albumartist%/[%year% - ]%album%/%tracknumber:2% %title%";
    let plan = app.preview_organise(&ids, pattern).unwrap();
    assert!(plan.iter().all(|p| p.status == "move"), "{plan:?}");
    assert!(
        plan[0]
            .to
            .contains("Sorted/Alpha Band/2001 - Greatest Hits/01 "),
        "{}",
        plan[0].to
    );
    assert!(
        std::path::Path::new(&before[0]).exists(),
        "a preview moves nothing"
    );

    let scans_before = scans();
    let batch = app.organise(&ids, pattern).unwrap();
    wait("moves", Duration::from_secs(20), || finished() == 1);
    wait("rescan", Duration::from_secs(20), || scans() > scans_before);
    for p in &plan {
        assert!(std::path::Path::new(&p.to).exists(), "{} moved", p.to);
        assert!(!std::path::Path::new(&p.from).exists());
    }
    assert!(
        !music.join("Alpha Band").exists(),
        "the emptied folder is removed"
    );
    let now = app.tracks(&ids).unwrap();
    assert_eq!(now.len(), ids.len(), "same track ids after the move");
    assert_eq!(now[0].play_count, 1, "play counts kept");
    assert_eq!(
        app.preview_organise(&ids, pattern).unwrap()[0].status,
        "same"
    );

    let scans_before = scans();
    app.undo_edit(batch).unwrap();
    wait("undo", Duration::from_secs(20), || finished() == 2);
    wait("rescan", Duration::from_secs(20), || scans() > scans_before);
    for path in &before {
        assert!(std::path::Path::new(path).exists(), "{path} back");
    }
    assert!(!music.join("Sorted").exists());
}
