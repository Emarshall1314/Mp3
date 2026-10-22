//! The owner's themes: one JSON file each in `<data>/themes`. Built-in themes live in the UI;
//! these are the ones made in the theme editor or imported.
//!
//! ```json
//! { "format": "cadenza-theme", "version": 1, "name": "Dusk", "scheme": "dark",
//!   "tokens": { "color-bg": "#14121f", "color-accent": "oklch(70% 0.15 300)" } }
//! ```
//!
//! Tokens are CSS custom properties without the leading `--`. Values are checked so a theme can
//! only set colours, sizes, fonts and gradients: no `url()` (a theme must never fetch anything)
//! and nothing that could end the declaration it is placed in.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{Error, Result};

const FORMAT: &str = "cadenza-theme";
const MAX_TOKENS: usize = 200;
const MAX_VALUE: usize = 300;

/// CSS functions a token value may use.
const FUNCTIONS: &[&str] = &[
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "hwb",
    "lab",
    "lch",
    "oklab",
    "oklch",
    "color",
    "color-mix",
    "linear-gradient",
    "radial-gradient",
    "conic-gradient",
    "repeating-linear-gradient",
    "repeating-radial-gradient",
    "var",
    "calc",
    "min",
    "max",
    "clamp",
    "cubic-bezier",
    "linear",
    "steps",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTheme {
    pub id: String,
    pub theme: Value,
}

pub(crate) struct Themes {
    dir: PathBuf,
}

impl Themes {
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("themes"),
        }
    }

    /// Every saved theme, by name. Files that aren't valid themes are skipped.
    pub(crate) fn list(&self) -> Vec<SavedTheme> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut themes: Vec<SavedTheme> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let id = path.file_stem()?.to_str()?.to_owned();
                if path.extension()? != "json" || !valid_id(&id) {
                    return None;
                }
                let value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
                Some(SavedTheme {
                    id,
                    theme: validate(&value).ok()?,
                })
            })
            .collect();
        themes.sort_by_key(|t| name(&t.theme).to_lowercase());
        themes
    }

    /// Saves a theme, replacing `id` or as a new one; returns it as stored.
    pub(crate) fn save(&self, id: Option<&str>, theme: &Value) -> Result<SavedTheme> {
        let theme = validate(theme).map_err(|e| Error::Other(format!("not a valid theme: {e}")))?;
        let id = match id {
            Some(id) if valid_id(id) => id.to_owned(),
            Some(id) => return Err(Error::Other(format!("“{id}” is not a theme id"))),
            None => self.new_id(name(&theme)),
        };
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        write_atomic(&self.dir.join(format!("{id}.json")), &theme)?;
        Ok(SavedTheme { id, theme })
    }

    /// Reads a theme file's text (from an import) and saves it as a new theme.
    pub(crate) fn import(&self, text: &str) -> Result<SavedTheme> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| Error::Other(format!("not a theme file: {e}")))?;
        self.save(None, &value)
    }

    pub(crate) fn delete(&self, id: &str) -> Result<()> {
        if !valid_id(id) {
            return Err(Error::Other(format!("“{id}” is not a theme id")));
        }
        match std::fs::remove_file(self.dir.join(format!("{id}.json"))) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        }
    }

    /// An id from the name ("Dusk" → "dusk", then "dusk-2", ...).
    fn new_id(&self, name: &str) -> String {
        let mut slug: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        slug = slug
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        slug.truncate(40);
        if slug.is_empty() {
            slug = "theme".into();
        }
        let taken = |id: &str| self.dir.join(format!("{id}.json")).exists();
        if !taken(&slug) {
            return slug;
        }
        (2..)
            .map(|n| format!("{slug}-{n}"))
            .find(|id| !taken(id))
            .unwrap_or_default()
    }
}

/// Writes a theme as a file for sharing (pretty JSON).
pub(crate) fn export(theme: &Value, path: &Path) -> Result<()> {
    let theme = validate(theme).map_err(|e| Error::Other(format!("not a valid theme: {e}")))?;
    write_atomic(path, &theme)
}

fn write_atomic(path: &Path, theme: &Value) -> Result<()> {
    let json = serde_json::to_vec_pretty(theme).map_err(|e| Error::Other(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

fn io(e: std::io::Error) -> Error {
    Error::Other(format!("saving the theme: {e}"))
}

fn name(theme: &Value) -> &str {
    theme["name"].as_str().unwrap_or_default()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The theme in its stored form, or what's wrong with it. Unknown top-level keys are dropped.
pub(crate) fn validate(value: &Value) -> std::result::Result<Value, String> {
    let obj = value.as_object().ok_or("expected a JSON object")?;
    if let Some(f) = obj.get("format")
        && f != FORMAT
    {
        return Err(format!("“format” should be “{FORMAT}”"));
    }
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .ok_or("it has no name")?;
    if name.chars().count() > 60 {
        return Err("the name is longer than 60 characters".into());
    }
    let scheme = match obj.get("scheme").and_then(Value::as_str) {
        Some(s @ ("light" | "dark")) => s,
        _ => return Err("“scheme” should be “light” or “dark”".into()),
    };
    let tokens = obj
        .get("tokens")
        .and_then(Value::as_object)
        .ok_or("it has no tokens")?;
    if tokens.len() > MAX_TOKENS {
        return Err(format!("more than {MAX_TOKENS} tokens"));
    }
    let mut out = Map::new();
    for (key, v) in tokens {
        if !valid_token_name(key) {
            return Err(format!("“{key}” is not a token name"));
        }
        let v = v
            .as_str()
            .ok_or_else(|| format!("“{key}” should be text"))?;
        check_value(v).map_err(|e| format!("“{key}”: {e}"))?;
        out.insert(key.clone(), Value::String(v.trim().to_owned()));
    }
    Ok(json!({
        "format": FORMAT,
        "version": 1,
        "name": name,
        "scheme": scheme,
        "tokens": out,
    }))
}

fn valid_token_name(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= 48
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn check_value(v: &str) -> std::result::Result<(), String> {
    if v.trim().is_empty() {
        return Err("empty".into());
    }
    if v.len() > MAX_VALUE {
        return Err(format!("longer than {MAX_VALUE} characters"));
    }
    if let Some(c) = v
        .chars()
        .find(|c| matches!(c, ';' | '{' | '}' | '<' | '>' | '\\' | '@' | '!') || c.is_control())
    {
        return Err(format!("“{c}” isn't allowed"));
    }
    // Every function call must be one of the allowed ones (so no url(), image() or attr()).
    let bytes = v.as_bytes();
    for (i, _) in v.match_indices('(') {
        let start = bytes[..i]
            .iter()
            .rposition(|b| !(b.is_ascii_alphanumeric() || *b == b'-'))
            .map_or(0, |p| p + 1);
        let function = v[start..i].to_ascii_lowercase();
        if !function.is_empty() && !FUNCTIONS.contains(&function.as_str()) {
            return Err(format!("{function}() isn't allowed"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(tokens: Value) -> Value {
        json!({ "name": "Dusk", "scheme": "dark", "tokens": tokens })
    }

    #[test]
    fn valid_themes_are_normalised() {
        let t = validate(&json!({
            "name": " Dusk ", "scheme": "dark", "extra": 1,
            "tokens": { "color-bg": " #14121f", "background-gradient": "linear-gradient(160deg, oklch(30% 0.1 300), rgb(0 0 0 / 50%))" }
        }))
        .unwrap();
        assert_eq!(t["name"], "Dusk");
        assert_eq!(t["format"], FORMAT);
        assert_eq!(t["tokens"]["color-bg"], "#14121f");
        assert!(t.get("extra").is_none());
    }

    #[test]
    fn unsafe_values_are_refused() {
        for bad in [
            "url(https://example.com/x.png)",
            "URL(x)",
            "red; background: red",
            "red }",
            "image-set(x)",
            "attr(x)",
            "#fff !important",
            "</style>",
            "\\75rl(x)",
        ] {
            assert!(
                validate(&theme(json!({ "color-bg": bad }))).is_err(),
                "{bad}"
            );
        }
        assert!(validate(&theme(json!({ "Color": "red" }))).is_err());
        assert!(validate(&theme(json!({ "color-bg": 3 }))).is_err());
        assert!(validate(&json!({ "name": "x", "scheme": "sepia", "tokens": {} })).is_err());
        assert!(validate(&json!({ "scheme": "dark", "tokens": {} })).is_err());
        assert!(validate(&json!([1])).is_err());
    }

    #[test]
    fn save_list_import_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let themes = Themes::new(dir.path());
        assert!(themes.list().is_empty());
        let a = themes
            .save(None, &theme(json!({ "color-bg": "#000" })))
            .unwrap();
        assert_eq!(a.id, "dusk");
        let b = themes
            .import(&serde_json::to_string(&theme(json!({}))).unwrap())
            .unwrap();
        assert_eq!(b.id, "dusk-2");
        themes
            .save(Some("dusk"), &theme(json!({ "color-bg": "#111" })))
            .unwrap();
        let list = themes.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].theme["tokens"]["color-bg"], "#111");
        assert!(themes.save(Some("../x"), &theme(json!({}))).is_err());
        assert!(themes.import("not json").is_err());
        themes.delete("dusk").unwrap();
        themes.delete("dusk").unwrap();
        assert_eq!(themes.list().len(), 1);

        let out = dir.path().join("shared.json");
        export(&list[1].theme, &out).unwrap();
        let back: Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
        assert_eq!(back, list[1].theme);
    }
}
