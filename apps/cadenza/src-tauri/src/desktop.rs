//! Desktop-only shell pieces: the tray (status-area) icon and the always-on-top mini player.

use std::sync::{Arc, Mutex};

use cadenza_api::{Cadenza, PlayerState, Status};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry};

pub const MINI_LABEL: &str = "mini";

/// The tray menu items that change with playback.
pub struct Tray {
    now: MenuItem<Wry>,
    toggle: MenuItem<Wry>,
    shown_track: Mutex<Option<i64>>,
}

/// Adds the tray icon. Desktops without a status-area host (StatusNotifier) simply don't show it.
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let now = MenuItem::with_id(app, "now", "Not Playing", false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Play", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &now,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &MenuItem::with_id(app, "next", "Next", true, None::<&str>)?,
            &MenuItem::with_id(app, "previous", "Previous", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "mini", "Mini Player", true, None::<&str>)?,
            &MenuItem::with_id(app, "show", "Show Cadenza", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
    )?;
    let icon = Image::from_bytes(include_bytes!("../icons/64x64.png"))?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Cadenza")
        .menu(&menu)
        .on_menu_event(|app, event| {
            let player = app.state::<Arc<Cadenza>>();
            match event.id().as_ref() {
                "toggle" => player.toggle(),
                "next" => player.next(),
                "previous" => player.previous(),
                "mini" => {
                    let _ = toggle_mini_player(app);
                }
                "show" => show_main(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;
    app.manage(Tray {
        now,
        toggle,
        shown_track: Mutex::new(None),
    });
    Ok(())
}

/// Keeps the tray menu in step with the player.
pub fn update_tray(app: &AppHandle, state: &PlayerState) {
    let (Some(tray), Some(player)) = (app.try_state::<Tray>(), app.try_state::<Arc<Cadenza>>())
    else {
        return;
    };
    let _ = tray.toggle.set_text(if state.status == Status::Playing {
        "Pause"
    } else {
        "Play"
    });
    let Ok(mut shown) = tray.shown_track.lock() else {
        return;
    };
    if *shown == state.track_id {
        return;
    }
    *shown = state.track_id;
    let text = state
        .track_id
        .and_then(|id| player.tracks(&[id]).ok())
        .and_then(|rows| rows.into_iter().next())
        .map(|t| format!("{} — {}", t.title, t.artist))
        .unwrap_or_else(|| "Not Playing".into());
    let _ = tray.now.set_text(text);
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Opens the mini player, or closes it if it is open.
pub fn toggle_mini_player(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(MINI_LABEL) {
        return w.close();
    }
    WebviewWindowBuilder::new(app, MINI_LABEL, WebviewUrl::App("index.html#mini".into()))
        .title("Cadenza Mini Player")
        .inner_size(380.0, 88.0)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .build()?;
    Ok(())
}
