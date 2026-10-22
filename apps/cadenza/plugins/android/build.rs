// The Kotlin half lives in `android/`; Tauri adds it to the generated Android project. The plugin
// has no commands of its own for the webview: the shell calls it from Rust.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
