//! A second "device" for trying sync on one machine: its own library and data folder, syncing
//! through the same folder as the app.
//!
//! ```text
//! cargo run -p cadenza-api --example sync_device -- <data dir> <music dir> <sync folder> <name> [title=<old>=<new>] [artist=<title>=<new>]
//! ```
//!
//! It scans the music folder, applies the edits given (by song title), syncs, waits for any tag
//! changes from other devices to be written, and syncs again.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use cadenza_api::{Cadenza, SongSort, SyncSettings, TrackEdit};

fn wait(app: &Cadenza, what: &str) {
    let start = Instant::now();
    std::thread::sleep(Duration::from_millis(300));
    while app.scan_status().running || app.edit_status().running {
        if start.elapsed() > Duration::from_secs(60) {
            eprintln!("gave up waiting for {what}");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, music, folder, name, edits @ ..] = args.as_slice() else {
        eprintln!(
            "usage: sync_device <data dir> <music dir> <sync folder> <name> [field=<title>=<value>...]"
        );
        std::process::exit(2);
    };
    let app = Cadenza::open(
        Path::new(data),
        Some(Box::new(cadenza_engine::Capture::new())),
        |_| {},
    )?;
    if app.roots()?.is_empty() {
        app.add_root(Path::new(music))?;
    }
    app.rescan();
    wait(&app, "the scan");
    app.set_sync_settings(SyncSettings {
        folder: Some(folder.clone()),
        device_name: name.clone(),
        ..Default::default()
    })?;
    for edit in edits {
        let mut parts = edit.splitn(3, '=');
        let (Some(field), Some(title), Some(value)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let song = app
            .songs(SongSort::Title, false, 0, 10_000)?
            .into_iter()
            .find(|t| t.title == title)
            .ok_or_else(|| format!("no song called {title}"))?;
        app.edit_tracks(
            vec![TrackEdit {
                track_id: song.id,
                fields: BTreeMap::from([(field.to_owned(), vec![value.to_owned()])]),
                pictures: None,
            }],
            "example edit",
        )?;
        wait(&app, "the edit");
        println!("{name}: set {field} of “{title}” to “{value}”");
    }
    let status = app.sync_blocking();
    wait(&app, "changes from other devices");
    let status2 = app.sync_blocking();
    println!(
        "{name}: synced ({} devices, {} conflicts{})",
        status2.devices.len(),
        status2.conflicts,
        status
            .error
            .map(|e| format!(", error: {e}"))
            .unwrap_or_default()
    );
    Ok(())
}
