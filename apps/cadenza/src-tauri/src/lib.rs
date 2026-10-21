//! Tauri shell: wires `cadenza-api` to the webview. No playback or library logic lives here.

#[tauri::command]
fn build_info() -> cadenza_api::BuildInfo {
    cadenza_api::build_info()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![build_info])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("cadenza: failed to start: {err}");
        std::process::exit(1);
    }
}
