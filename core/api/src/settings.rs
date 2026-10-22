//! Per-device settings (this machine's outputs, not the library): `settings.json` in the data
//! folder. Written atomically, so a crash never leaves a half-written file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Output device id (see `cadenza_engine::output::devices`).
    pub output_device: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_device: cadenza_engine::output::DEFAULT_DEVICE.to_owned(),
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

    /// The saved settings; defaults if there are none or the file is unreadable.
    pub(crate) fn load(&self) -> Settings {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub(crate) fn save(&self, settings: &Settings) -> Result<()> {
        let io = |e: std::io::Error| Error::Other(format!("saving settings: {e}"));
        let json = serde_json::to_vec_pretty(settings).map_err(|e| Error::Other(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(io)?;
        std::fs::rename(&tmp, &self.path).map_err(io)
    }
}
