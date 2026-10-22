//! Tauri shell: wires `cadenza-api` to the webview. No playback or library logic lives here.
//!
//! Commands that touch the database run off the main thread (`async`), events are pushed to the
//! UI as `cadenza://event`, and artwork is served by the `cadenza` URI scheme from the
//! thumbnail cache: `cadenza://localhost/art/<hash>?s=<px>`.

use std::sync::Arc;

use cadenza_api::{
    AlbumDetail, AlbumRow, AlbumSort, ApiEvent, ArtistRow, BuildInfo, Cadenza, LibraryStats,
    Outputs, PlayerState, Repeat, Root, SavedSearch, ScanStatus, SearchResults, SongSort, TrackRow,
};
use tauri::http::{Response, StatusCode, header};
use tauri::{AppHandle, Emitter, Manager, State};

type App<'a> = State<'a, Arc<Cadenza>>;
type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

#[tauri::command]
fn build_info() -> BuildInfo {
    cadenza_api::build_info()
}

#[tauri::command(async)]
fn roots(app: App<'_>) -> Res<Vec<Root>> {
    app.roots().map_err(e)
}

#[tauri::command(async)]
fn add_root(app: App<'_>, path: String) -> Res<Root> {
    app.add_root(std::path::Path::new(&path)).map_err(e)
}

#[tauri::command(async)]
fn remove_root(app: App<'_>, id: i64) -> Res<()> {
    app.remove_root(id).map_err(e)
}

#[tauri::command]
fn rescan(app: App<'_>) {
    app.rescan();
}

#[tauri::command]
fn scan_status(app: App<'_>) -> ScanStatus {
    app.scan_status()
}

#[tauri::command(async)]
fn stats(app: App<'_>) -> Res<LibraryStats> {
    app.stats().map_err(e)
}

#[tauri::command(async)]
fn songs(app: App<'_>, sort: SongSort, desc: bool, offset: i64, limit: i64) -> Res<Vec<TrackRow>> {
    app.songs(sort, desc, offset, limit).map_err(e)
}

#[tauri::command(async)]
fn albums(app: App<'_>, sort: AlbumSort, offset: i64, limit: i64) -> Res<Vec<AlbumRow>> {
    app.albums(sort, offset, limit).map_err(e)
}

#[tauri::command(async)]
fn album(app: App<'_>, id: i64) -> Res<AlbumDetail> {
    app.album(id).map_err(e)
}

#[tauri::command(async)]
fn artists(app: App<'_>) -> Res<Vec<ArtistRow>> {
    app.artists().map_err(e)
}

#[tauri::command(async)]
fn artist_albums(app: App<'_>, name: String) -> Res<Vec<AlbumRow>> {
    app.artist_albums(&name).map_err(e)
}

#[tauri::command(async)]
fn search(app: App<'_>, query: String, limit: i64) -> Res<SearchResults> {
    app.search(&query, limit).map_err(e)
}

#[tauri::command(async)]
fn saved_searches(app: App<'_>) -> Res<Vec<SavedSearch>> {
    app.saved_searches().map_err(e)
}

#[tauri::command(async)]
fn save_search(app: App<'_>, name: String, query: String) -> Res<SavedSearch> {
    app.save_search(&name, &query).map_err(e)
}

#[tauri::command(async)]
fn delete_saved_search(app: App<'_>, id: i64) -> Res<()> {
    app.delete_saved_search(id).map_err(e)
}

#[tauri::command(async)]
fn play_tracks(app: App<'_>, ids: Vec<i64>, start: usize, shuffle: bool) -> Res<()> {
    app.play_tracks(&ids, start, shuffle).map_err(e)
}

#[tauri::command(async)]
fn play_album(app: App<'_>, album_id: i64, start: usize, shuffle: bool) -> Res<()> {
    app.play_album(album_id, start, shuffle).map_err(e)
}

#[tauri::command(async)]
fn play_songs(app: App<'_>, sort: SongSort, desc: bool, index: usize, shuffle: bool) -> Res<()> {
    app.play_songs(sort, desc, index, shuffle).map_err(e)
}

#[tauri::command(async)]
fn play_next(app: App<'_>, ids: Vec<i64>) -> Res<()> {
    app.play_next(&ids).map_err(e)
}

#[tauri::command(async)]
fn play_later(app: App<'_>, ids: Vec<i64>) -> Res<()> {
    app.play_later(&ids).map_err(e)
}

#[tauri::command]
fn player(app: App<'_>) -> PlayerState {
    app.player()
}

#[tauri::command(async)]
fn queue_window(app: App<'_>, offset: usize, limit: usize) -> Res<Vec<TrackRow>> {
    app.queue_window(offset, limit).map_err(e)
}

#[tauri::command(async)]
fn tracks(app: App<'_>, ids: Vec<i64>) -> Res<Vec<TrackRow>> {
    app.tracks(&ids).map_err(e)
}

#[tauri::command]
fn toggle(app: App<'_>) {
    app.toggle();
}

#[tauri::command]
fn next(app: App<'_>) {
    app.next();
}

#[tauri::command]
fn previous(app: App<'_>) {
    app.previous();
}

#[tauri::command]
fn seek(app: App<'_>, position_ms: u64) {
    app.seek(position_ms);
}

#[tauri::command]
fn set_volume(app: App<'_>, volume: f32) {
    app.set_volume(volume);
}

#[tauri::command(async)]
fn outputs(app: App<'_>) -> Outputs {
    app.outputs()
}

#[tauri::command(async)]
fn set_output(app: App<'_>, id: String) -> Res<()> {
    app.set_output(&id).map_err(e)
}

#[tauri::command]
fn play_index(app: App<'_>, index: usize) {
    app.play_index(index);
}

#[tauri::command]
fn remove_from_queue(app: App<'_>, index: usize) {
    app.remove_from_queue(index);
}

#[tauri::command]
fn move_in_queue(app: App<'_>, from: usize, to: usize) {
    app.move_in_queue(from, to);
}

#[tauri::command]
fn clear_upcoming(app: App<'_>) {
    app.clear_upcoming();
}

#[tauri::command]
fn set_repeat(app: App<'_>, repeat: Repeat) {
    app.set_repeat(repeat);
}

#[tauri::command]
fn set_shuffle(app: App<'_>, on: bool) {
    app.set_shuffle(on);
}

/// `/art/<hash>?s=<px>` → JPEG thumbnail from the cache (generated on first request).
fn artwork_response(app: &AppHandle, uri: &tauri::http::Uri) -> Response<Vec<u8>> {
    let not_found = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap_or_default()
    };
    let Some(hash) = uri.path().strip_prefix("/art/") else {
        return not_found();
    };
    let size = uri
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("s=")))
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    let Some(cadenza) = app.try_state::<Arc<Cadenza>>() else {
        return not_found();
    };
    match cadenza
        .artwork(hash, size)
        .and_then(|p| std::fs::read(p).map_err(|e| cadenza_api::Error::Other(e.to_string())))
    {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "image/jpeg")
            // Content-addressed: a hash never changes meaning.
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .body(bytes)
            .unwrap_or_default(),
        Err(_) => not_found(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("cadenza", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || responder.respond(artwork_response(&app, request.uri())));
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let handle = app.handle().clone();
            let cadenza = Cadenza::open(&data_dir, None, move |event: ApiEvent| {
                let _ = handle.emit("cadenza://event", event);
            })?;
            // `cadenza --add-folder <path>`: add a music folder from the command line or a script.
            let args: Vec<String> = std::env::args().collect();
            for pair in args.windows(2).filter(|w| w[0] == "--add-folder") {
                if let Err(err) = cadenza.add_root(std::path::Path::new(&pair[1])) {
                    eprintln!("cadenza: cannot add {}: {err}", pair[1]);
                }
            }
            app.manage(Arc::new(cadenza));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            build_info,
            roots,
            add_root,
            remove_root,
            rescan,
            scan_status,
            stats,
            songs,
            albums,
            album,
            artists,
            artist_albums,
            search,
            saved_searches,
            save_search,
            delete_saved_search,
            play_tracks,
            play_album,
            play_songs,
            play_next,
            play_later,
            player,
            queue_window,
            tracks,
            toggle,
            next,
            previous,
            seek,
            set_volume,
            play_index,
            remove_from_queue,
            move_in_queue,
            clear_upcoming,
            set_repeat,
            set_shuffle,
            outputs,
            set_output,
        ])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("cadenza: failed to start: {err}");
        std::process::exit(1);
    }
}
