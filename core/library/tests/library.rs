//! Library behaviour against the fixture library: grouping, scanning, moves, CUE, search.

#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cadenza_library::{AlbumSort, Library, ScanReport, SongSort};
use cadenza_testkit::{fixtures_dir, manifest};

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

struct Setup {
    _tmp: tempfile::TempDir,
    music: PathBuf,
    lib: Library,
}

fn setup(groups: &[&str]) -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    for g in groups {
        copy_dir(&fixtures_dir().join(g), &music.join(g));
    }
    let lib = Library::open(&tmp.path().join("data")).unwrap();
    lib.add_root(&music).unwrap();
    Setup {
        _tmp: tmp,
        music,
        lib,
    }
}

fn scan(lib: &Library) -> ScanReport {
    let r = lib.scan(&|_| {}).unwrap();
    assert!(r.errors.is_empty(), "scan errors: {:?}", r.errors);
    r
}

#[test]
fn albums_group_by_artist_title_and_release() {
    let s = setup(&["library"]);
    let report = scan(&s.lib);
    let expected_tracks = manifest()
        .entries
        .iter()
        .filter(|e| e.group == "library")
        .count() as u64;
    assert_eq!(report.added, expected_tracks);

    // Entries sharing an album_key land in one album; different keys never merge.
    let songs = s.lib.songs(SongSort::Artist, false, 0, 1000).unwrap();
    let ids: Vec<i64> = songs.iter().map(|t| t.id).collect();
    let album_of: HashMap<i64, Option<i64>> = songs.iter().map(|t| (t.id, t.album_id)).collect();
    let by_path: HashMap<PathBuf, i64> = s
        .lib
        .playable(&ids)
        .unwrap()
        .into_iter()
        .map(|p| (p.path, p.id))
        .collect();
    let mut key_to_album: HashMap<String, i64> = HashMap::new();
    for e in manifest()
        .entries
        .iter()
        .filter(|e| e.group == "library" && e.album_key.is_some())
    {
        let id = by_path[&s.music.join(&e.path)];
        let album_id = album_of[&id].unwrap();
        let key = e.album_key.clone().unwrap();
        if let Some(prev) = key_to_album.insert(key.clone(), album_id) {
            assert_eq!(prev, album_id, "{key}: tracks split across albums");
        }
    }
    let distinct: std::collections::HashSet<i64> = key_to_album.values().copied().collect();
    assert_eq!(
        distinct.len(),
        key_to_album.len(),
        "different album keys merged"
    );

    let albums = s.lib.albums(AlbumSort::Title, 0, 100).unwrap();
    let hits: Vec<_> = albums
        .iter()
        .filter(|a| a.title == "Greatest Hits")
        .collect();
    assert_eq!(
        hits.len(),
        2,
        "same-named albums by different artists must stay apart"
    );

    // Editions: separate albums linked as other versions.
    let night = albums
        .iter()
        .filter(|a| a.title == "Night Drive")
        .collect::<Vec<_>>();
    assert_eq!(night.len(), 2);
    let detail = s.lib.album(night[0].id).unwrap();
    assert_eq!(detail.other_versions.len(), 1);
    assert_eq!(detail.other_versions[0].id, night[1].id);

    // Compilation and multi-disc classical album.
    let mix = albums.iter().find(|a| a.title == "Summer Mix").unwrap();
    assert_eq!(mix.albumartist, "Various Artists");
    assert!(mix.compilation);
    assert_eq!(mix.track_count, 3);
    let sym = albums.iter().find(|a| a.title == "Symphony No. 1").unwrap();
    let detail = s.lib.album(sym.id).unwrap();
    assert_eq!(detail.disc_count, 2);
    let order: Vec<_> = detail.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        order,
        ["I. Allegro", "II. Adagio", "III. Scherzo", "IV. Finale"]
    );

    // Untagged files get their titles from file names.
    assert!(songs.iter().any(|t| t.title == "Some Artist - Some Song"));
}

#[test]
fn artwork_is_cached_and_thumbnailed() {
    let s = setup(&["library", "artwork"]);
    scan(&s.lib);
    let albums = s.lib.albums(AlbumSort::Title, 0, 100).unwrap();
    let hits = albums.iter().find(|a| a.title == "Greatest Hits").unwrap();
    let hash = hits.art_hash.clone().expect("embedded art");
    let thumb = s.lib.artwork_thumbnail(&hash, 200).unwrap();
    let img = image_size(&thumb);
    assert_eq!(img, (256, 256), "600px cover → 256px thumbnail");
    // Folder art when nothing is embedded: cover.jpg wins over folder.png.
    let folder = albums
        .iter()
        .find(|a| a.title == "Folder Art Album")
        .unwrap();
    let original = s
        .lib
        .artwork_original(folder.art_hash.as_ref().unwrap())
        .unwrap();
    let cover = std::fs::read(s.music.join("artwork/folder_art_album/cover.jpg")).unwrap();
    assert_eq!(std::fs::read(original).unwrap(), cover);
    // A truncated embedded JPEG is ignored rather than cached.
    let songs = s.lib.search("path:mp3_truncated_jpeg", 10).unwrap().songs;
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].art_hash, None);
}

fn image_size(path: &Path) -> (u32, u32) {
    // JPEG SOF0/SOF2 scan: enough for our own thumbnails.
    let b = std::fs::read(path).unwrap();
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] == 0xff && (b[i + 1] == 0xc0 || b[i + 1] == 0xc2) {
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            return (w, h);
        }
        i += 1;
    }
    panic!("not a JPEG")
}

#[test]
fn rescans_are_incremental_and_follow_moves() {
    let s = setup(&["library"]);
    let first = scan(&s.lib);
    let again = scan(&s.lib);
    assert_eq!(
        (again.added, again.updated, again.moved, again.missing),
        (0, 0, 0, 0)
    );
    assert_eq!(again.unchanged, first.added);

    let song = s
        .lib
        .search("title:\"First Hit\" artist:Alpha", 5)
        .unwrap()
        .songs
        .remove(0);
    s.lib.record_play(song.id, 1000, true, None).unwrap();

    // Move the file to another folder: same track (uid, play count), new path.
    let from = s
        .music
        .join("library/Alpha Band/2001 - Greatest Hits/01 First Hit.flac");
    let to_dir = s.music.join("library/Moved Here");
    std::fs::create_dir_all(&to_dir).unwrap();
    std::fs::rename(&from, to_dir.join("renamed.flac")).unwrap();
    let r = scan(&s.lib);
    assert_eq!((r.added, r.moved, r.missing), (0, 1, 0));
    let after = s.lib.tracks(&[song.id]).unwrap().remove(0);
    assert_eq!(after.uid, song.uid);
    assert_eq!(after.play_count, 1);
    assert!(
        s.lib.playable(&[song.id]).unwrap()[0]
            .path
            .ends_with("Moved Here/renamed.flac")
    );

    // Delete: hidden, not forgotten. Restore: the same track comes back.
    let gone = to_dir.join("renamed.flac");
    let bytes = std::fs::read(&gone).unwrap();
    std::fs::remove_file(&gone).unwrap();
    let r = scan(&s.lib);
    assert_eq!(r.missing, 1);
    assert!(s.lib.tracks(&[song.id]).unwrap()[0].uid == song.uid);
    assert!(
        s.lib
            .search("\"First Hit\"", 5)
            .unwrap()
            .songs
            .iter()
            .all(|t| t.id != song.id)
    );
    std::fs::write(&gone, bytes).unwrap();
    let r = scan(&s.lib);
    assert_eq!((r.added, r.updated), (0, 1));
    assert_eq!(s.lib.tracks(&[song.id]).unwrap()[0].play_count, 1);
}

#[test]
fn cue_sheets_split_single_file_albums() {
    let s = setup(&["cue"]);
    scan(&s.lib);
    let albums = s.lib.albums(AlbumSort::Title, 0, 10).unwrap();
    assert_eq!(albums.len(), 3, "{albums:?}");
    for a in &albums {
        let d = s.lib.album(a.id).unwrap();
        assert_eq!(d.tracks.len(), 3, "{}: {:?}", a.title, d.tracks);
        assert_eq!(d.tracks[0].title, "Première");
        let total: i64 = d.tracks.iter().map(|t| t.duration_ms).sum();
        assert!((5990..=6010).contains(&total), "{total}");
    }
    let titles: Vec<String> = s
        .lib
        .songs(SongSort::Title, false, 0, 20)
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert!(titles.iter().any(|t| t == "二番目"));
    assert!(
        titles.iter().any(|t| t == "Deuxième"),
        "CP1252 sheet decoded: {titles:?}"
    );
    let p = s
        .lib
        .playable(&s.lib.song_ids(SongSort::Title, false).unwrap())
        .unwrap();
    assert!(p.iter().all(|t| t.start_frame.is_some()));
}

#[test]
fn search_syntax_finds_the_right_tracks() {
    let s = setup(&["library", "formats"]);
    scan(&s.lib);
    let count = |q: &str| s.lib.search(q, 1000).unwrap().song_count;
    assert_eq!(count("greatest"), 5);
    assert_eq!(count("artist:\"Alpha Band\""), 3);
    assert_eq!(count("artist:=gamma"), 5);
    assert_eq!(count("year:>2009 artist:gamma"), 5);
    assert_eq!(count("year:2010"), 2);
    assert_eq!(count("year:1990..2002"), 5);
    assert_eq!(count("night -demo"), 4);
    assert_eq!(count("format:dsd"), 7); // DSF ×4, DFF ×2, WavPack DSD
    assert_eq!(count("quality:hires format:wavpack"), 3);
    assert_eq!(count("rate:>=352.8 format:flac"), 2);
    assert_eq!(count("bits:32"), 6); // WAV s32/f32×2, FLAC s32, WavPack f32/s32
    assert!(count("added:<1") > 0);
    // Accent- and case-insensitive substring matching.
    assert_eq!(count("CONCERTO"), 0);
    assert_eq!(count("allegro"), 1);
    let r = s.lib.search("greatest", 10).unwrap();
    assert_eq!(r.albums.len(), 2);
    let r = s.lib.search("gam", 10).unwrap();
    assert_eq!(r.artists.len(), 1);

    let saved = s
        .lib
        .save_search("Hi-res FLAC", "format:flac quality:hires")
        .unwrap();
    assert_eq!(s.lib.saved_searches().unwrap(), vec![saved.clone()]);
    s.lib.delete_saved_search(saved.id).unwrap();
    assert!(s.lib.saved_searches().unwrap().is_empty());
}

#[test]
fn whole_fixture_library_scans_without_crashing() {
    let s = setup(&[
        "formats",
        "tags",
        "encodings",
        "artwork",
        "cue",
        "corrupt",
        "library",
        "multichannel",
        "gapless",
    ]);
    let r = s.lib.scan(&|_| {}).unwrap();
    // Corrupt and unsupported files are reported, never fatal.
    assert!(r.failed > 0 && r.failed < 12, "{r:?}");
    assert!(r.added > 120, "{r:?}");
    let stats = s.lib.stats().unwrap();
    assert_eq!(stats.tracks as u64, r.added);
}

#[test]
fn replay_gain_comes_from_tags_and_measurements_survive_rescans() {
    use cadenza_library::{Gain, MeasuredGain, SongSort};

    let s = setup(&["tags", "formats"]);
    scan(&s.lib);
    let ids = s.lib.song_ids(SongSort::Title, false).unwrap();
    let tracks = s.lib.playable(&ids).unwrap();
    let tagged: Vec<_> = tracks
        .iter()
        .filter(|t| t.path.to_string_lossy().contains("/tags/"))
        .filter(|t| t.gain.track_gain.is_some())
        .collect();
    assert!(tagged.len() >= 10, "only {} tagged files", tagged.len());
    for t in &tagged {
        assert_eq!(
            t.gain,
            Gain {
                track_gain: Some(-7.89),
                track_peak: Some(0.987_654),
                album_gain: Some(-8.12),
                album_peak: Some(0.999),
            },
            "{}",
            t.path.display()
        );
    }
    // The untagged format fixtures need measuring.
    let todo = s.lib.tracks_missing_gain().unwrap();
    assert!(
        todo.iter()
            .all(|t| t.track.path.to_string_lossy().contains("/formats/"))
    );
    let first = todo[0].track.id;
    let measured = Gain {
        track_gain: Some(-3.5),
        track_peak: Some(0.5),
        album_gain: None,
        album_peak: None,
    };
    s.lib
        .store_measured_gain(vec![MeasuredGain {
            track_id: first,
            gain: measured,
        }])
        .unwrap();
    // A rescan that finds the file unchanged keeps the measurement.
    s.lib.scan(&|_| {}).unwrap();
    assert_eq!(s.lib.playable(&[first]).unwrap()[0].gain, measured);
    assert_eq!(s.lib.tracks_missing_gain().unwrap().len(), todo.len() - 1);
}

#[test]
fn lyrics_come_from_sidecar_lrc_or_tags() {
    use cadenza_library::SongSort;

    let s = setup(&["tags", "library"]);
    scan(&s.lib);
    let ids = s.lib.song_ids(SongSort::Title, false).unwrap();
    let tracks = s.lib.playable(&ids).unwrap();
    let mut embedded = 0;
    for t in &tracks {
        let path = t.path.to_string_lossy();
        let lyrics = s.lib.lyrics(t.id).unwrap();
        if path.ends_with("Lyric Song.flac") || path.contains("Lyric Song.") {
            let l = lyrics.expect("sidecar lyrics");
            assert_eq!(l.source, "file");
            assert!(l.synced);
            assert!(l.lines.windows(2).all(|w| w[0].time_ms <= w[1].time_ms));
        } else if path.contains("/tags/") {
            let l = lyrics.unwrap_or_else(|| panic!("{path}: no lyrics"));
            assert_eq!(l.source, "embedded");
            let text: Vec<&str> = l.lines.iter().map(|l| l.text.as_str()).collect();
            assert_eq!(text, ["First line", "Second line"], "{path}");
            embedded += 1;
        }
    }
    assert!(embedded >= 13, "{embedded}");
}

#[test]
fn artwork_palettes_find_distinct_dominant_colours() {
    use cadenza_library::AlbumSort;

    let s = setup(&["library"]);
    scan(&s.lib);
    let albums = s.lib.albums(AlbumSort::Artist, 0, 100).unwrap();
    let with_art: Vec<_> = albums.iter().filter_map(|a| a.art_hash.clone()).collect();
    assert!(!with_art.is_empty());
    for hash in with_art {
        let p = s.lib.artwork_palette(&hash).unwrap();
        assert!(!p.is_empty() && p.len() <= 4, "{p:?}");
        for (i, a) in p.iter().enumerate() {
            for b in &p[i + 1..] {
                let d: i32 = (0..3)
                    .map(|k| (i32::from(a[k]) - i32::from(b[k])).pow(2))
                    .sum();
                assert!(d > 3600, "{p:?}");
            }
        }
    }
}

#[test]
fn artist_pages_and_home_reflect_releases_and_plays() {
    use cadenza_library::AlbumSort;

    let s = setup(&["library"]);
    scan(&s.lib);
    let gamma = s.lib.artist_page("gamma").unwrap();
    assert_eq!(gamma.sections.len(), 1);
    assert_eq!(gamma.sections[0].kind, "albums");
    assert_eq!(gamma.sections[0].albums.len(), 2, "both editions");
    assert_eq!(gamma.sections[0].albums[0].year, Some(2020), "newest first");
    assert!(gamma.top_songs.is_empty(), "nothing played yet");

    // Delta appears only on the Various Artists compilation.
    let delta = s.lib.artist_page("Delta").unwrap();
    assert!(delta.sections.is_empty());
    assert_eq!(delta.appears_on.len(), 1);
    assert_eq!(delta.appears_on[0].title, "Summer Mix");

    // Plays drive top songs and the Home shelves.
    let night = &gamma.sections[0].albums[1];
    let tracks = s.lib.album(night.id).unwrap().tracks;
    s.lib.record_play(tracks[1].id, 1000, true, None).unwrap();
    s.lib.record_play(tracks[1].id, 1000, true, None).unwrap();
    s.lib.record_play(tracks[0].id, 1000, true, None).unwrap();
    s.lib.record_play(tracks[0].id, 500, false, None).unwrap();
    let gamma = s.lib.artist_page("Gamma").unwrap();
    let top: Vec<i64> = gamma.top_songs.iter().map(|t| t.id).collect();
    assert_eq!(top, [tracks[1].id, tracks[0].id]);

    let home = s.lib.home().unwrap();
    assert_eq!(home.recently_played.len(), 1);
    assert_eq!(home.recently_played[0].id, night.id);
    assert_eq!(home.most_played[0].id, night.id);
    let added = s.lib.albums(AlbumSort::Added, 0, 12).unwrap();
    assert_eq!(home.recently_added, added);
}

#[test]
fn column_browser_narrows_by_genre_then_artist() {
    let s = setup(&["library"]);
    scan(&s.lib);
    let all = s.lib.browse(None, None).unwrap();
    let total: i64 = all.genres.iter().map(|g| g.album_count).sum();
    assert_eq!(
        total as usize,
        all.albums.len(),
        "every album is in one genre"
    );
    assert_eq!(
        all.genres.last().map(|g| g.name.as_str()),
        Some(""),
        "no genre last"
    );

    let genre = all.genres.iter().find(|g| !g.name.is_empty()).unwrap();
    let in_genre = s.lib.browse(Some(&genre.name), None).unwrap();
    assert_eq!(in_genre.albums.len() as i64, genre.album_count);
    assert_eq!(in_genre.genres, all.genres, "the genre column stays whole");

    let artist = &in_genre.artists[0];
    let narrowed = s.lib.browse(Some(&genre.name), Some(&artist.name)).unwrap();
    assert_eq!(narrowed.albums.len() as i64, artist.album_count);
    assert!(
        narrowed
            .albums
            .iter()
            .all(|a| a.albumartist.eq_ignore_ascii_case(&artist.name))
    );
    let by_artist = s.lib.browse(None, Some("gamma")).unwrap();
    assert_eq!(by_artist.albums.len(), 2, "both Night Drive editions");
}

#[test]
fn favourites_and_imported_listens_count_once() {
    let s = setup(&["library"]);
    scan(&s.lib);
    let songs = s.lib.songs(SongSort::Title, false, 0, 3).unwrap();
    let (a, b) = (songs[0].id, songs[1].id);
    s.lib.set_favourite(&[a], true).unwrap();
    assert!(s.lib.tracks(&[a]).unwrap()[0].favourite);
    let hash = s.lib.audio_hashes(&[a]).unwrap()[0].1.clone();
    assert!(s.lib.tracks_with_hash(&hash).unwrap().contains(&a));
    assert_eq!(s.lib.favourite_hashes().unwrap(), [hash]);
    s.lib.set_favourite(&[a], false).unwrap();
    assert!(s.lib.favourite_hashes().unwrap().is_empty());

    s.lib.record_play(b, 1000, true, None).unwrap();
    s.lib
        .record_play(b, 1000, true, Some("1.0.x".into()))
        .unwrap();
    let unsynced = s.lib.unsynced_plays().unwrap();
    assert_eq!(unsynced.len(), 1);
    s.lib
        .mark_plays_synced(vec![(unsynced[0].id, "2.0.x".into())])
        .unwrap();
    assert!(s.lib.unsynced_plays().unwrap().is_empty());

    assert!(s.lib.import_play(b, 100, 1000, true, "3.0.y").unwrap());
    assert!(
        !s.lib.import_play(b, 100, 1000, true, "3.0.y").unwrap(),
        "only once"
    );
    assert!(
        !s.lib.import_play(b, 100, 1000, true, "1.0.x").unwrap(),
        "own listen"
    );
    assert_eq!(s.lib.tracks(&[b]).unwrap()[0].play_count, 3);
}

#[test]
fn trash_hides_a_song_and_restore_brings_it_back_with_its_history() {
    let s = setup(&["library"]);
    scan(&s.lib);
    let song = s.lib.songs(SongSort::Title, false, 0, 1).unwrap()[0].clone();
    let path = s.lib.playable(&[song.id]).unwrap()[0].path.clone();
    std::fs::write(path.with_extension("lrc"), "[00:01.00]hi").unwrap();
    s.lib.record_play(song.id, 1000, true, None).unwrap();
    let before = s.lib.stats().unwrap().tracks;

    let items = s.lib.trash(&[song.id]).unwrap();
    assert_eq!(items.len(), 1);
    assert!(!path.exists() && !path.with_extension("lrc").exists());
    assert_eq!(s.lib.stats().unwrap().tracks, before - 1);
    assert!(s.lib.playable(&[song.id]).unwrap().is_empty());
    assert_eq!(s.lib.trash_items().unwrap()[0].title, song.title);
    // A scan doesn't bring it back, and nothing is old enough to purge.
    scan(&s.lib);
    assert_eq!(s.lib.stats().unwrap().tracks, before - 1);
    assert_eq!(s.lib.purge_expired().unwrap(), 0);

    s.lib.restore(items[0].id).unwrap();
    assert!(path.exists() && path.with_extension("lrc").exists());
    scan(&s.lib);
    assert_eq!(s.lib.stats().unwrap().tracks, before);
    let back = s.lib.tracks(&[song.id]).unwrap();
    assert_eq!(back[0].play_count, 1, "same song, same history");
    assert!(s.lib.trash_items().unwrap().is_empty());

    // Purging deletes the file for good.
    let items = s.lib.trash(&[song.id]).unwrap();
    s.lib.purge(items[0].id).unwrap();
    assert!(s.lib.trash_items().unwrap().is_empty());
    assert!(!path.exists());
    assert!(s.lib.restore(items[0].id).is_err());
}

#[test]
fn rediscover_brings_back_old_favourites_and_unplayed_albums() {
    use cadenza_library::AlbumSort;

    let s = setup(&["library"]);
    scan(&s.lib);
    let albums = s.lib.albums(AlbumSort::Artist, 0, 100).unwrap();
    // Play every song of the first albums three times, now.
    let mut played = Vec::new();
    for a in albums.iter().take(4) {
        for t in s.lib.album(a.id).unwrap().tracks {
            for _ in 0..3 {
                s.lib.record_play(t.id, 1000, true, None).unwrap();
            }
            played.push(t.id);
        }
    }
    assert!(played.len() >= 6, "enough songs for a mix");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Today: nothing is forgotten yet, and nothing has waited long enough to be "still unplayed".
    let today = s.lib.rediscover(now, 0).unwrap();
    assert!(today.mixes.iter().all(|m| m.id != "forgotten"));
    assert!(today.unplayed.is_empty());
    assert_eq!(
        today,
        s.lib.rediscover(now + 60, 0).unwrap(),
        "fixed for the day"
    );

    // A hundred days on: the played songs are forgotten favourites; the other albums wait.
    let later = s.lib.rediscover(now + 100 * 86_400, 0).unwrap();
    let forgotten = later
        .mixes
        .iter()
        .find(|m| m.id == "forgotten")
        .expect("a mix");
    assert!(forgotten.track_ids.iter().all(|id| played.contains(id)));
    assert!(!forgotten.art.is_empty());
    let first: Vec<i64> = albums.iter().take(4).map(|a| a.id).collect();
    assert!(!later.unplayed.is_empty());
    assert!(later.unplayed.iter().all(|a| !first.contains(&a.id)));
    assert!(later.album_of_the_day.is_some());
}

#[test]
fn playlists_keep_order_through_adds_moves_and_removals() {
    use cadenza_library::AlbumSort;

    let s = setup(&["library"]);
    scan(&s.lib);
    let albums = s.lib.albums(AlbumSort::Artist, 0, 100).unwrap();
    let songs: Vec<i64> = albums
        .iter()
        .flat_map(|a| s.lib.album(a.id).unwrap().tracks)
        .map(|t| t.id)
        .take(5)
        .collect();
    assert_eq!(songs.len(), 5);
    let order = |id: i64| -> Vec<i64> {
        s.lib
            .playlist(id)
            .unwrap()
            .items
            .iter()
            .map(|i| i.track.id)
            .collect()
    };

    let p = s.lib.create_playlist("  Road Trip ", &songs[..3]).unwrap();
    assert_eq!(p.name, "Road Trip");
    assert_eq!(p.track_count, 3);
    assert_eq!(order(p.id), &songs[..3]);

    // Adding appends, duplicates included.
    s.lib.add_to_playlist(p.id, &[songs[3], songs[0]]).unwrap();
    assert_eq!(
        order(p.id),
        [songs[0], songs[1], songs[2], songs[3], songs[0]]
    );

    // Move the last item to the front, and the first to the end.
    let items = s.lib.playlist(p.id).unwrap().items;
    s.lib
        .move_in_playlist(p.id, items[4].item_id, Some(items[0].item_id))
        .unwrap();
    assert_eq!(
        order(p.id),
        [songs[0], songs[0], songs[1], songs[2], songs[3]]
    );
    let items = s.lib.playlist(p.id).unwrap().items;
    s.lib
        .move_in_playlist(p.id, items[2].item_id, None)
        .unwrap();
    assert_eq!(
        order(p.id),
        [songs[0], songs[0], songs[2], songs[3], songs[1]]
    );

    // Remove one of the duplicates (by item, not by song).
    let items = s.lib.playlist(p.id).unwrap().items;
    s.lib
        .remove_from_playlist(p.id, &[items[0].item_id])
        .unwrap();
    assert_eq!(order(p.id), [songs[0], songs[2], songs[3], songs[1]]);

    // Rename; an empty name falls back; the newest-changed playlist comes first.
    let q = s.lib.create_playlist("", &[]).unwrap();
    assert_eq!(q.name, "Untitled Playlist");
    assert_eq!(q.track_count, 0);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    s.lib.rename_playlist(p.id, "Summer Road Trip").unwrap();
    let all = s.lib.playlists().unwrap();
    assert_eq!(all[0].name, "Summer Road Trip");
    assert!(!all[0].art.is_empty());

    s.lib.delete_playlist(q.id).unwrap();
    assert_eq!(s.lib.playlists().unwrap().len(), 1);
    assert!(s.lib.playlist(q.id).is_err());
    assert!(s.lib.add_to_playlist(q.id, &songs).is_err());
}

#[test]
fn letter_starts_follow_the_list_order() {
    use cadenza_library::{AlbumSort, SongSort};

    let s = setup(&["library"]);
    scan(&s.lib);
    for sort in [AlbumSort::Artist, AlbumSort::Title] {
        let albums = s.lib.albums(sort, 0, 1000).unwrap();
        let letters = s.lib.album_letters(sort).unwrap();
        assert!(!letters.is_empty());
        // Each letter's first album is at its index, and starts are in list order.
        for w in letters.windows(2) {
            assert!(w[0].index < w[1].index);
        }
        for l in &letters {
            let a = &albums[l.index as usize];
            let name = if sort == AlbumSort::Artist {
                &a.albumartist
            } else {
                &a.title
            };
            let first = name
                .trim_start_matches("The ")
                .chars()
                .next()
                .unwrap_or('#');
            if first.is_ascii_alphabetic() {
                assert_eq!(l.letter, first.to_ascii_uppercase().to_string(), "{name}");
            }
        }
    }
    assert!(s.lib.album_letters(AlbumSort::Added).unwrap().is_empty());
    let songs = s.lib.song_letters(SongSort::Title, false).unwrap();
    assert_eq!(songs[0].index, 0);
    assert!(
        s.lib
            .song_letters(SongSort::Duration, false)
            .unwrap()
            .is_empty()
    );
}
