//! Access to the generated test library (`cargo xtask fixtures`) for Cadenza's tests.
//!
//! The manifest is the oracle: `tools/fixtures/check.py` has already verified every entry against
//! ffmpeg, metaflac and mutagen, so tests can compare Cadenza's results with it directly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

/// Directory holding the fixtures: `$CADENZA_FIXTURES`, else `<workspace>/fixtures-out`.
pub fn fixtures_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CADENZA_FIXTURES") {
        return PathBuf::from(dir);
    }
    // core/testkit -> workspace root
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out")
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub generator_version: u32,
    pub tools: BTreeMap<String, String>,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Expect {
    /// Reads and plays completely.
    Ok,
    /// Readable with damage (e.g. truncated audio or broken artwork); must not crash.
    Partial,
    /// Not a usable audio file; must be rejected cleanly.
    Error,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub id: String,
    pub path: String,
    pub group: String,
    pub container: String,
    pub codec: String,
    pub lossless: bool,
    pub channels: u32,
    pub sample_rate: Option<u32>,
    pub bits: Option<u32>,
    /// `s` signed int, `f` float, `dsd`.
    pub sample_format: Option<String>,
    /// Sample frames after gapless trimming (DSD: 1-bit samples per channel).
    pub frames: Option<u64>,
    /// MD5 of the PCM in the canonical layout (see `tools/fixtures/generate.py`).
    pub pcm_md5: Option<String>,
    pub dsd_rate: Option<u32>,
    pub dsd_md5: Option<String>,
    /// Expected values by Cadenza field name (`performer:<role>`, `custom:<NAME>` for those kinds).
    pub tags: Option<BTreeMap<String, Vec<String>>>,
    /// The native tag format and items as written.
    pub native: Option<serde_json::Value>,
    #[serde(default)]
    pub preserve: Vec<serde_json::Value>,
    #[serde(default)]
    pub artwork: Vec<Artwork>,
    pub gapless: Option<Gapless>,
    pub cue: Option<serde_json::Value>,
    pub album_key: Option<String>,
    pub expect: Expect,
    #[serde(default)]
    pub notes: String,
}

impl Entry {
    pub fn file(&self) -> PathBuf {
        fixtures_dir().join(&self.path)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artwork {
    /// ID3/FLAC picture type (3 = front cover).
    #[serde(rename = "type")]
    pub picture_type: u8,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub sha1: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Gapless {
    pub set: String,
    pub index: u32,
    pub count: u32,
    pub source_frames: u64,
    pub source_md5: String,
    pub start_frame: u64,
}

/// The parsed manifest. Panics with instructions if the fixtures have not been generated, so a
/// missing library is a loud failure rather than silently skipped tests.
pub fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let path = fixtures_dir().join("manifest.json");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "fixture manifest {} unreadable ({e}); run `cargo xtask fixtures` first",
                path.display()
            )
        });
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("fixture manifest {} invalid: {e}", path.display()))
    })
}

/// All entries in a group (`formats`, `gapless`, `tags`, ...).
pub fn group(name: &str) -> impl Iterator<Item = &'static Entry> {
    let name = name.to_owned();
    manifest().entries.iter().filter(move |e| e.group == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::unwrap_used)]
    fn manifest_parses_and_every_file_exists() {
        let m = manifest();
        assert!(m.entries.len() > 100, "suspiciously small fixture library");
        for e in &m.entries {
            assert!(e.file().exists(), "{} missing", e.path);
        }
        for g in [
            "formats",
            "gapless",
            "tags",
            "encodings",
            "artwork",
            "cue",
            "corrupt",
            "library",
        ] {
            assert!(group(g).next().is_some(), "group {g} empty");
        }
        let flac24 = m
            .entries
            .iter()
            .find(|e| e.id == "formats/flac_s24_96000")
            .unwrap();
        assert_eq!(
            (flac24.sample_rate, flac24.bits, flac24.frames),
            (Some(96000), Some(24), Some(96000))
        );
    }
}
