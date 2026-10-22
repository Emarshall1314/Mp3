//! Tauri shell: wires `cadenza-api` to the webview. No playback or library logic lives here.
//!
//! Commands that touch the database run off the main thread (`async`), events are pushed to the
//! UI as `cadenza://event`, and artwork is served by the `cadenza` URI scheme from the
//! thumbnail cache: `cadenza://localhost/art/<hash>?s=<px>`.

use std::path::PathBuf;
use std::sync::Arc;

use cadenza_api::{
    AlbumDetail, AlbumRow, AlbumSort, ApiEvent, Appearance, ArtistPage, ArtistRow, Browse,
    BuildInfo, Cadenza, DspSettings, EditBatch, EditChange, EditStatus, EqBand, Home, ImportedEq,
    LibraryStats, LoudnessStatus, Lyrics, Outputs, PlayerState, RenamePlan, Repeat,
    ReplayGainSettings, Root, SavedSearch, SavedTheme, ScanStatus, SearchResults, Settings,
    SongSort, StoredArt, TrackEdit, TrackRow, TrackTags,
};
use tauri::http::{Response, StatusCode, header};
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(target_os = "android")]
mod android;
#[cfg(desktop)]
mod desktop;
#[cfg(target_os = "linux")]
mod mpris;

type App<'a> = State<'a, Arc<Cadenza>>;
type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

#[tauri::command]
fn build_info() -> BuildInfo {
    cadenza_api::build_info()
}

/// `cadenza --safe-mode`: start without the owner's custom CSS, to undo CSS that hides the way
/// back to Settings.
#[tauri::command]
fn safe_mode() -> bool {
    std::env::args().any(|a| a == "--safe-mode")
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
fn browse(app: App<'_>, genre: Option<String>, artist: Option<String>) -> Res<Browse> {
    app.browse(genre.as_deref(), artist.as_deref()).map_err(e)
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

#[tauri::command(async)]
fn set_dop(app: App<'_>, id: String, on: bool) -> Res<()> {
    app.set_dop(&id, on).map_err(e)
}

#[tauri::command]
fn replay_gain(app: App<'_>) -> ReplayGainSettings {
    app.replay_gain()
}

#[tauri::command(async)]
fn set_replay_gain(app: App<'_>, settings: ReplayGainSettings) -> Res<()> {
    app.set_replay_gain(settings).map_err(e)
}

#[tauri::command]
fn settings(app: App<'_>) -> Settings {
    app.settings()
}

#[tauri::command(async)]
fn set_crossfade(app: App<'_>, seconds: f64) -> Res<()> {
    app.set_crossfade(seconds).map_err(e)
}

#[tauri::command(async)]
fn track_tags(app: App<'_>, ids: Vec<i64>) -> Res<Vec<TrackTags>> {
    app.track_tags(&ids).map_err(e)
}

#[tauri::command(async)]
fn edit_tracks(app: App<'_>, edits: Vec<TrackEdit>, description: String) -> Res<i64> {
    app.edit_tracks(edits, &description).map_err(e)
}

#[tauri::command]
fn edit_status(app: App<'_>) -> EditStatus {
    app.edit_status()
}

#[tauri::command]
fn cancel_edit(app: App<'_>) {
    app.cancel_edit();
}

#[tauri::command(async)]
fn edit_history(app: App<'_>, limit: i64) -> Res<Vec<EditBatch>> {
    app.edit_history(limit).map_err(e)
}

#[tauri::command(async)]
fn edit_changes(app: App<'_>, batch: i64) -> Res<Vec<EditChange>> {
    app.edit_changes(batch).map_err(e)
}

#[tauri::command(async)]
fn undo_edit(app: App<'_>, batch: i64) -> Res<i64> {
    app.undo_edit(batch).map_err(e)
}

/// An image file into the artwork cache, for a picture edit.
#[tauri::command(async)]
fn import_artwork(app: App<'_>, path: String) -> Res<StoredArt> {
    let path = tauri_plugin_cadenza_android::local_path(&path)?;
    app.import_artwork(std::path::Path::new(&path)).map_err(e)
}

#[tauri::command(async)]
fn save_cover_to_folder(app: App<'_>, track_id: i64) -> Res<String> {
    app.save_cover_to_folder(track_id)
        .map(|p| p.display().to_string())
        .map_err(e)
}

#[tauri::command(async)]
fn preview_organise(app: App<'_>, ids: Vec<i64>, pattern: String) -> Res<Vec<RenamePlan>> {
    app.preview_organise(&ids, &pattern).map_err(e)
}

#[tauri::command(async)]
fn organise(app: App<'_>, ids: Vec<i64>, pattern: String) -> Res<i64> {
    app.organise(&ids, &pattern).map_err(e)
}

#[tauri::command(async)]
fn set_tag_backups(app: App<'_>, on: bool) -> Res<()> {
    app.set_tag_backups(on).map_err(e)
}

/// Saves the appearance and tells every window (main and mini player) to apply it.
#[tauri::command(async)]
fn set_appearance(app: App<'_>, handle: AppHandle, appearance: Appearance) -> Res<()> {
    app.set_appearance(appearance.clone()).map_err(e)?;
    let _ = handle.emit("cadenza://appearance", appearance);
    Ok(())
}

#[tauri::command(async)]
fn themes(app: App<'_>) -> Vec<SavedTheme> {
    app.themes()
}

#[tauri::command(async)]
fn save_theme(
    app: App<'_>,
    handle: AppHandle,
    id: Option<String>,
    theme: serde_json::Value,
) -> Res<SavedTheme> {
    let saved = app.save_theme(id.as_deref(), &theme).map_err(e)?;
    let _ = handle.emit("cadenza://themes", ());
    Ok(saved)
}

#[tauri::command(async)]
fn import_theme(app: App<'_>, handle: AppHandle, text: String) -> Res<SavedTheme> {
    let saved = app.import_theme(&text).map_err(e)?;
    let _ = handle.emit("cadenza://themes", ());
    Ok(saved)
}

#[tauri::command(async)]
fn delete_theme(app: App<'_>, handle: AppHandle, id: String) -> Res<()> {
    app.delete_theme(&id).map_err(e)?;
    let _ = handle.emit("cadenza://themes", ());
    Ok(())
}

/// Writes a theme file to `path`: from a save dialog on Linux, in the Download folder on Android
/// (where save dialogs give content URIs, not paths).
#[tauri::command(async)]
fn export_theme(app: App<'_>, theme: serde_json::Value, path: String) -> Res<String> {
    let path = PathBuf::from(path);
    app.export_theme(&theme, &path).map_err(e)?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn loudness_status(app: App<'_>) -> LoudnessStatus {
    app.loudness_status()
}

#[tauri::command]
fn analyze_loudness(app: App<'_>) {
    app.analyze_loudness();
}

#[tauri::command(async)]
fn artist_page(app: App<'_>, name: String) -> Res<ArtistPage> {
    app.artist_page(&name).map_err(e)
}

#[tauri::command(async)]
fn home(app: App<'_>) -> Res<Home> {
    app.home().map_err(e)
}

#[tauri::command(async)]
fn lyrics(app: App<'_>, track_id: i64) -> Res<Option<Lyrics>> {
    app.lyrics(track_id).map_err(e)
}

#[tauri::command(async)]
fn artwork_palette(app: App<'_>, hash: String) -> Res<Vec<[u8; 3]>> {
    app.artwork_palette(&hash).map_err(e)
}

/// Opens or closes the always-on-top mini player.
#[tauri::command]
fn toggle_mini_player(app: tauri::AppHandle) -> Res<()> {
    #[cfg(desktop)]
    {
        desktop::toggle_mini_player(&app).map_err(e)
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        Err("there is no mini player on this platform".into())
    }
}

/// Whether Cadenza may read music folders. Always on desktop; Android asks for file access.
#[tauri::command(async)]
fn storage_access(handle: AppHandle) -> Res<bool> {
    #[cfg(target_os = "android")]
    {
        storage(&handle).has_access().map_err(e)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = handle;
        Ok(true)
    }
}

/// Asks for file access (Android's settings page); true once granted.
#[tauri::command(async)]
fn request_storage_access(handle: AppHandle) -> Res<bool> {
    #[cfg(target_os = "android")]
    {
        storage(&handle).request_access().map_err(e)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = handle;
        Ok(true)
    }
}

/// Android's folder picker, as a path (`None` if cancelled). Desktop uses the dialog plugin.
#[tauri::command(async)]
fn pick_music_folder(handle: AppHandle) -> Res<Option<String>> {
    #[cfg(target_os = "android")]
    {
        storage(&handle).pick_folder().map_err(e)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = handle;
        Err("use the folder dialog on this platform".into())
    }
}

#[cfg(target_os = "android")]
fn storage(handle: &AppHandle) -> State<'_, tauri_plugin_cadenza_android::Storage<tauri::Wry>> {
    handle.state()
}

/// Brings the main window to the front.
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    #[cfg(desktop)]
    desktop::show_main(&app);
    #[cfg(not(desktop))]
    let _ = app;
}

#[tauri::command]
fn dsp(app: App<'_>) -> DspSettings {
    app.dsp()
}

#[tauri::command(async)]
fn set_dsp(app: App<'_>, mut settings: DspSettings) -> Res<()> {
    if let Some(path) = &settings.convolution_path {
        settings.convolution_path = Some(tauri_plugin_cadenza_android::local_path(path)?);
    }
    app.set_dsp(settings).map_err(e)
}

#[tauri::command(async)]
fn import_autoeq(app: App<'_>, path: String) -> Res<ImportedEq> {
    let path = tauri_plugin_cadenza_android::local_path(&path)?;
    app.import_autoeq(std::path::Path::new(&path)).map_err(e)
}

#[tauri::command]
fn eq_curve(app: App<'_>, preamp_db: f64, bands: Vec<EqBand>, freqs: Vec<f64>) -> Vec<f64> {
    app.eq_curve(preamp_db, &bands, &freqs)
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
        .plugin(tauri_plugin_cadenza_android::init())
        .register_asynchronous_uri_scheme_protocol("cadenza", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || responder.respond(artwork_response(&app, request.uri())));
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let handle = app.handle().clone();
            // Player events also go to the system media controls (MPRIS, Android's media session).
            let (to_media, media_events) = std::sync::mpsc::channel();
            let cadenza = Cadenza::open(&data_dir, None, move |event: ApiEvent| {
                if let ApiEvent::Player { event } = &event {
                    #[cfg(desktop)]
                    if let cadenza_api::PlayerEvent::State(state) = event {
                        desktop::update_tray(&handle, state);
                    }
                    let _ = to_media.send(event.clone());
                }
                let _ = handle.emit("cadenza://event", event);
            })?;
            // `cadenza --add-folder <path>`: add a music folder from the command line or a script.
            let args: Vec<String> = std::env::args().collect();
            for pair in args.windows(2).filter(|w| w[0] == "--add-folder") {
                if let Err(err) = cadenza.add_root(std::path::Path::new(&pair[1])) {
                    eprintln!("cadenza: cannot add {}: {err}", pair[1]);
                }
            }
            let cadenza = Arc::new(cadenza);
            #[cfg(target_os = "linux")]
            {
                let (app_ref, handle) = (Arc::clone(&cadenza), app.handle().clone());
                std::thread::Builder::new()
                    .name("cadenza-mpris".into())
                    .spawn(move || mpris::run(app_ref, handle, media_events))?;
            }
            // Android: the playback service's media session (lock screen, notification).
            #[cfg(target_os = "android")]
            {
                let app_ref = Arc::clone(&cadenza);
                std::thread::Builder::new()
                    .name("cadenza-session".into())
                    .spawn(move || android::run(app_ref, media_events))?;
            }
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            drop(media_events);
            app.manage(cadenza);
            #[cfg(desktop)]
            if let Err(err) = desktop::setup_tray(app.handle()) {
                eprintln!("cadenza: no tray icon: {err}");
            }
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
            set_dop,
            artist_page,
            browse,
            home,
            lyrics,
            artwork_palette,
            toggle_mini_player,
            show_main_window,
            dsp,
            set_dsp,
            replay_gain,
            set_replay_gain,
            loudness_status,
            settings,
            set_crossfade,
            set_appearance,
            safe_mode,
            themes,
            save_theme,
            import_theme,
            delete_theme,
            export_theme,
            track_tags,
            edit_tracks,
            edit_status,
            cancel_edit,
            edit_history,
            edit_changes,
            undo_edit,
            set_tag_backups,
            preview_organise,
            organise,
            import_artwork,
            save_cover_to_folder,
            storage_access,
            request_storage_access,
            pick_music_folder,
            analyze_loudness,
            import_autoeq,
            eq_curve,
        ])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("cadenza: failed to start: {err}");
        std::process::exit(1);
    }
}
