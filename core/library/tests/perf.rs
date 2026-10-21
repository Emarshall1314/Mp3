//! Performance budgets on a synthetic library (run by `cargo xtask bench`).
//!
//! `CADENZA_PERF_TRACKS` sets the size (default 200 000). Files are tiny WAVs with RIFF INFO tags
//! and unique audio, laid out as artist/album/track like a real collection.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::time::{Duration, Instant};

use cadenza_library::{AlbumSort, Library, SongSort};

const WORDS: &[&str] = &[
    "radio", "night", "blue", "river", "echo", "glass", "summer", "silver", "ghost", "paper",
    "golden", "wild", "ocean", "neon", "velvet", "storm", "garden", "stone", "electric",
    "midnight", "crystal", "fire", "dream", "shadow", "light",
];

fn word(i: usize) -> &'static str {
    WORDS[i % WORDS.len()]
}

fn wav(path: &Path, title: &str, artist: &str, album: &str, track: usize, seed: u32) {
    let mut info = b"INFO".to_vec();
    for (id, text) in [
        (b"INAM", title),
        (b"IART", artist),
        (b"IPRD", album),
        (b"ITRK", &track.to_string()),
    ] {
        let mut data = text.as_bytes().to_vec();
        data.push(0);
        info.extend_from_slice(id);
        info.extend_from_slice(&(data.len() as u32).to_le_bytes());
        info.extend_from_slice(&data);
        if data.len() % 2 == 1 {
            info.push(0);
        }
    }
    let frames = 441u32; // 10 ms, mono 16-bit
    let mut pcm = Vec::with_capacity(frames as usize * 2);
    let mut x = seed.wrapping_mul(2_654_435_761);
    for _ in 0..frames {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        pcm.extend_from_slice(&((x >> 20) as i16).to_le_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(4 + 24 + 8 + info.len() as u32 + 8 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&[1, 0, 1, 0]);
    out.extend_from_slice(&44_100u32.to_le_bytes());
    out.extend_from_slice(&88_200u32.to_le_bytes());
    out.extend_from_slice(&[2, 0, 16, 0]);
    out.extend_from_slice(b"LIST");
    out.extend_from_slice(&(info.len() as u32).to_le_bytes());
    out.extend_from_slice(&info);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(&pcm);
    std::fs::write(path, out).unwrap();
}

fn report(name: &str, d: Duration, budget: Duration) {
    let ok = if d <= budget { "ok" } else { "OVER BUDGET" };
    println!(
        "{name:<44} {:>9.1} ms   budget {:>7.0} ms   {ok}",
        d.as_secs_f64() * 1000.0,
        budget.as_secs_f64() * 1000.0
    );
}

#[test]
#[ignore = "benchmark: cargo xtask bench"]
fn budgets_at_library_scale() {
    let n: usize = std::env::var("CADENZA_PERF_TRACKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200_000);
    let base = std::env::var_os("CADENZA_PERF_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/perf-library"));
    let music = base.join(format!("music-{n}"));
    if !music.join(".complete").exists() {
        let t = Instant::now();
        let _ = std::fs::remove_dir_all(&music);
        // 12 tracks per album, 8 albums per artist.
        for i in 0..n {
            let (artist, album, track) = (i / 96, i / 12, i % 12 + 1);
            let dir = music
                .join(format!("Artist {artist} {}", word(artist)))
                .join(format!("Album {album} {}", word(album / 3)));
            if track == 1 {
                std::fs::create_dir_all(&dir).unwrap();
            }
            let title = format!("{} {} {i}", word(i), word(i / 7));
            wav(
                &dir.join(format!("{track:02}.wav")),
                &title,
                &format!("Artist {artist} {}", word(artist)),
                &format!("Album {album} {}", word(album / 3)),
                track,
                i as u32,
            );
        }
        std::fs::write(music.join(".complete"), b"").unwrap();
        println!("generated {n} files in {:.1} s", t.elapsed().as_secs_f64());
    }
    let data = base.join(format!("data-{n}"));
    let _ = std::fs::remove_dir_all(&data);
    let lib = Library::open(&data).unwrap();
    lib.add_root(&music).unwrap();

    let t = Instant::now();
    let r = lib.scan(&|_| {}).unwrap();
    let first = t.elapsed();
    assert_eq!(r.added as usize, n, "{r:?}");
    println!(
        "first scan: {n} tracks in {:.1} s ({:.0} tracks/s)",
        first.as_secs_f64(),
        n as f64 / first.as_secs_f64()
    );

    let t = Instant::now();
    let r = lib.scan(&|_| {}).unwrap();
    assert_eq!(r.unchanged as usize, n);
    report(
        "incremental rescan, nothing changed",
        t.elapsed(),
        Duration::from_secs(5),
    );

    // Cold open (what app start pays for the library).
    drop(lib);
    let t = Instant::now();
    let lib = Library::open(&data).unwrap();
    let stats = lib.stats().unwrap();
    report(
        "open library + stats",
        t.elapsed(),
        Duration::from_millis(300),
    );
    assert_eq!(stats.tracks as usize, n);

    for q in [
        "radio",
        "midnight crystal",
        "artist:velvet",
        "year:>2000 format:pcm",
        "electric -dream",
        "glass 1234",
    ] {
        let _ = lib.search(q, 200).unwrap(); // warm
        let t = Instant::now();
        let reps = 5;
        for _ in 0..reps {
            lib.search(q, 200).unwrap();
        }
        report(
            &format!("search {q:?}"),
            t.elapsed() / reps,
            Duration::from_millis(50),
        );
    }

    for (name, offset) in [
        ("songs page at start", 0),
        ("songs page deep (offset n·3/4)", n * 3 / 4),
    ] {
        let t = Instant::now();
        let rows = lib
            .songs(SongSort::Artist, false, offset as i64, 100)
            .unwrap();
        assert_eq!(rows.len(), 100);
        report(name, t.elapsed(), Duration::from_millis(50));
    }
    let t = Instant::now();
    lib.albums(AlbumSort::Artist, (n / 24) as i64, 100).unwrap();
    report("albums page", t.elapsed(), Duration::from_millis(50));
    let t = Instant::now();
    let ids = lib.song_ids(SongSort::Title, false).unwrap();
    assert_eq!(ids.len(), n);
    report(
        "all song ids sorted by title",
        t.elapsed(),
        Duration::from_millis(500),
    );
}
