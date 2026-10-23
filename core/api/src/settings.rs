//! Per-device settings (this machine's outputs, not the library): `settings.json` in the data
//! folder. Written atomically, so a crash never leaves a half-written file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cadenza_engine::{DspSettings, ReplayGainSettings};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Output device id (see `cadenza_engine::output::devices`).
    pub output_device: String,
    /// Devices that get DSD as DoP. Off unless the owner turns it on: a DAC without DoP support
    /// would play it as full-scale noise.
    pub dop_devices: Vec<String>,
    /// DSP profile per output device id: headphones and speakers want different EQ.
    pub dsp: BTreeMap<String, DspSettings>,
    pub replay_gain: ReplayGainSettings,
    /// Crossfade between tracks, in seconds (0: off).
    pub crossfade_s: f64,
    pub appearance: Appearance,
    /// Copy each file to `<data>/backups` before writing its tags.
    pub tag_backups: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follow the desktop's light/dark preference.
    #[default]
    Auto,
    Light,
    Dark,
}

/// How the app looks. The reduce switches add to the desktop's own accessibility preferences:
/// on, they always apply; off, the desktop's preference still does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    pub theme: Theme,
    pub reduce_motion: bool,
    pub reduce_transparency: bool,
    /// The rest of the look and layout (themes chosen, accent, background, density, layout,
    /// shortcuts...), owned by the UI, which fills in defaults. Stored as given.
    pub look: serde_json::Value,
}

impl Settings {
    pub fn dop(&self, device: &str) -> bool {
        self.dop_devices.iter().any(|d| d == device)
    }

    /// The current output device's DSP profile.
    pub fn current_dsp(&self) -> DspSettings {
        self.dsp
            .get(&self.output_device)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_device: cadenza_engine::output::DEFAULT_DEVICE.to_owned(),
            dop_devices: Vec::new(),
            dsp: BTreeMap::new(),
            replay_gain: ReplayGainSettings::default(),
            crossfade_s: 0.0,
            appearance: Appearance::default(),
            tag_backups: false,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Store {
    path: PathBuf,
}

impl Store {
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("settings.json"),
        }
    }

    /// The saved settings, and anything about them that couldn't be read.
    ///
    /// One unreadable value never costs the rest: each setting that reads is kept and only the
    /// ones that don't fall back to their defaults. If the file can't be read at all, a copy is
    /// kept beside it (`settings.json.unreadable`) before anything saves over it.
    pub(crate) fn load(&self) -> (Settings, Vec<String>) {
        let bytes = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (Settings::default(), Vec::new());
            }
            Err(e) => {
                return (
                    Settings::default(),
                    vec![format!("couldn't read settings: {e}")],
                );
            }
        };
        if let Ok(s) = serde_json::from_slice::<Settings>(&bytes) {
            return (s, Vec::new());
        }
        let keep = |why: String| {
            let _ = std::fs::write(self.path.with_extension("json.unreadable"), &bytes);
            why
        };
        let Ok(serde_json::Value::Object(saved)) = serde_json::from_slice(&bytes) else {
            let why = keep("the settings file was damaged; defaults are in use and a copy of it is kept as settings.json.unreadable".into());
            return (Settings::default(), vec![why]);
        };
        // Start from the defaults and take each saved setting that reads.
        let mut merged = match serde_json::to_value(Settings::default()) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => return (Settings::default(), Vec::new()),
        };
        let mut lost = Vec::new();
        for (key, value) in saved {
            let mut trial = merged.clone();
            trial.insert(key.clone(), value);
            if serde_json::from_value::<Settings>(serde_json::Value::Object(trial.clone())).is_ok()
            {
                merged = trial;
            } else {
                lost.push(key);
            }
        }
        let settings =
            serde_json::from_value(serde_json::Value::Object(merged)).unwrap_or_default();
        let why = keep(format!(
            "some settings couldn't be read and are back to their defaults ({}); a copy of the old file is kept as settings.json.unreadable",
            lost.join(", ")
        ));
        (settings, vec![why])
    }

    pub(crate) fn save(&self, settings: &Settings) -> Result<()> {
        let io = |e: std::io::Error| Error::Other(format!("saving settings: {e}"));
        let json = serde_json::to_vec_pretty(settings).map_err(|e| Error::Other(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(io)?;
        std::fs::rename(&tmp, &self.path).map_err(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        (dir, store)
    }

    #[test]
    fn settings_survive_a_save_and_load() {
        let (_dir, store) = store();
        let mut s = Settings::default();
        s.appearance.look = serde_json::json!({ "darkTheme": "forest", "accent": "#f0731f" });
        s.crossfade_s = 2.5;
        store.save(&s).expect("save");
        let (back, problems) = store.load();
        assert_eq!(back, s);
        assert!(problems.is_empty());
    }

    #[test]
    fn one_unreadable_setting_does_not_reset_the_look() {
        let (dir, store) = store();
        let mut s = Settings::default();
        s.appearance.look = serde_json::json!({ "darkTheme": "forest" });
        store.save(&s).expect("save");
        // A value this build can't read (an older or newer format, or a damaged one).
        let mut v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("settings.json")).expect("read"))
                .expect("json");
        v["crossfadeS"] = serde_json::json!("not a number");
        std::fs::write(dir.path().join("settings.json"), v.to_string()).expect("write");

        let (back, problems) = store.load();
        assert_eq!(
            back.appearance.look["darkTheme"], "forest",
            "the look survives"
        );
        assert_eq!(
            back.crossfade_s, 0.0,
            "only the bad value is back to its default"
        );
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("crossfadeS"), "{problems:?}");
        assert!(dir.path().join("settings.json.unreadable").exists());
    }

    #[test]
    fn a_damaged_file_is_kept_not_overwritten() {
        let (dir, store) = store();
        std::fs::write(dir.path().join("settings.json"), b"{ not json").expect("write");
        let (back, problems) = store.load();
        assert_eq!(back, Settings::default());
        assert_eq!(problems.len(), 1);
        assert_eq!(
            std::fs::read(dir.path().join("settings.json.unreadable")).expect("kept"),
            b"{ not json"
        );
        // A missing file is simply a first run.
        std::fs::remove_file(dir.path().join("settings.json")).expect("rm");
        assert!(store.load().1.is_empty());
    }
}
