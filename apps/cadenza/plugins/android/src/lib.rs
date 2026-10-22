//! Android pieces of the Cadenza shell, as a Tauri plugin with a Kotlin half (`android/`):
//!
//! - **Storage access.** Cadenza reads music folders as ordinary files, so it asks for "All files
//!   access" (`MANAGE_EXTERNAL_STORAGE`; `READ_EXTERNAL_STORAGE` before Android 11). Media-only
//!   access would hide `.lrc`, `.cue` and folder artwork.
//! - **The music folder picker** (the system folder picker, returned as a file path).
//! - **The playback service**: a foreground service with a media session, so playback continues
//!   with the screen off and shows lock-screen and notification controls. It talks to the core
//!   over JNI (`MediaBridge`, implemented by the app crate), not through this plugin, because it
//!   outlives the activity that plugin calls go through.
//!
//! - **[`local_path`]**: file paths for the `content://` URIs Android's file pickers return.
//!
//! On other platforms the plugin does nothing, and `local_path` passes paths through.

use tauri::Runtime;
use tauri::plugin::{Builder, TauriPlugin};

#[cfg(target_os = "android")]
mod android;
mod paths;

#[cfg(target_os = "android")]
pub use android::Storage;
pub use paths::local_path;

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("cadenza-android")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = api.register_android_plugin("app.cadenza.android", "CadenzaPlugin")?;
                app.manage(Storage(handle));
            }
            #[cfg(not(target_os = "android"))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}
