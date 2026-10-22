//! System-wide shortcuts (Linux, X11): keys that work while another app has focus. The UI
//! chooses them; pressing one sends `cadenza://global-shortcut` with the action's id, and the UI
//! runs it. Wayland doesn't let apps grab keys, so there they can't be registered; desktops bind
//! keys to `playerctl` instead, which reaches Cadenza through MPRIS.

use std::str::FromStr;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyResult {
    /// Wayland session: system-wide shortcuts can't work.
    pub wayland: bool,
    /// Actions whose keys couldn't be registered, with why.
    pub failed: Vec<(String, String)>,
}

pub fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// A combination as the UI stores it ("Ctrl+Alt+ArrowRight", "Ctrl+,") in the plugin's terms.
pub fn accelerator(combo: &str) -> Option<Shortcut> {
    let mut parts: Vec<&str> = combo.split('+').collect();
    // "Ctrl++" ends with an empty part for the plus key itself.
    if combo.ends_with("++") {
        parts.retain(|p| !p.is_empty());
        parts.push("Equal");
    }
    let key = parts.pop()?;
    let key = match key {
        "," => "Comma",
        "." => "Period",
        "/" => "Slash",
        ";" => "Semicolon",
        "'" => "Quote",
        "[" => "BracketLeft",
        "]" => "BracketRight",
        "-" => "Minus",
        "=" => "Equal",
        "`" => "Backquote",
        "\\" => "Backslash",
        k => k,
    };
    let text = parts
        .iter()
        .map(|m| if *m == "Ctrl" { "Control" } else { m })
        .chain(std::iter::once(key))
        .collect::<Vec<_>>()
        .join("+");
    Shortcut::from_str(&text).ok()
}

/// Replaces the registered shortcuts with `bindings` (action id, combination).
pub fn set(app: &AppHandle, bindings: Vec<(String, String)>) -> HotkeyResult {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    let mut failed = Vec::new();
    for (action, combo) in bindings {
        let Some(shortcut) = accelerator(&combo) else {
            failed.push((action, format!("“{combo}” can’t be used system-wide")));
            continue;
        };
        let id = action.clone();
        let registered = shortcuts.on_shortcut(shortcut, move |app, _, event| {
            if event.state() == ShortcutState::Pressed {
                let _ = app.emit("cadenza://global-shortcut", &id);
            }
        });
        if let Err(e) = registered {
            failed.push((action, e.to_string()));
        }
    }
    HotkeyResult {
        wayland: is_wayland(),
        failed,
    }
}

#[cfg(test)]
mod tests {
    use super::accelerator;

    #[test]
    fn ui_combinations_become_accelerators() {
        for combo in [
            "Ctrl+Alt+ArrowRight",
            "Ctrl+Shift+P",
            "Alt+Space",
            "Ctrl+,",
            "Ctrl+Alt+MediaPlayPause",
            "F9",
        ] {
            assert!(accelerator(combo).is_some(), "{combo}");
        }
        assert_eq!(accelerator("Ctrl+Alt+P"), accelerator("Control+Alt+P"));
        assert!(accelerator("Ctrl+Nonsense").is_none());
    }
}
