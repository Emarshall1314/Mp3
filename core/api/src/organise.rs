//! File names from tags, for the organiser: a pattern such as
//! `%albumartist%/[%year% - ]%album%/[%discnumber%-]%tracknumber:2% %title%` becomes a path
//! relative to the music folder.
//!
//! - `%field%` is any field ("albumartist", "title", "custom:NAME"); several values are joined
//!   as the library shows them ("A, B & C").
//! - `%field:N%` pads a number to N digits, or cuts text to N characters.
//! - `%year%` is the first four characters of the date.
//! - `[...]` is left out when a field inside it is empty.
//! - `/` separates folders.
//!
//! Names are made safe for every filesystem Cadenza's files may sit on, Android SD cards (FAT,
//! exFAT) included: characters those reject are replaced, trailing dots and spaces dropped, and
//! each name kept under 255 bytes.

use std::path::PathBuf;

use cadenza_tags::Fields;

/// Longest name (one folder or file) in bytes, leaving room for the extension.
const MAX_NAME: usize = 200;

/// The relative path for a song with `fields`, keeping its extension (lower-cased). An error
/// names what's missing when the pattern gives an empty name.
pub(crate) fn relative_path(
    pattern: &str,
    fields: &Fields,
    extension: &str,
) -> Result<PathBuf, String> {
    if pattern.trim().is_empty() {
        return Err("the pattern is empty".into());
    }
    let mut path = PathBuf::new();
    let parts: Vec<&str> = pattern.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        let name = sanitise(&expand(part, fields)?);
        if name.is_empty() {
            return Err(format!("“{part}” gives an empty name for this song"));
        }
        if i + 1 == parts.len() {
            let ext = extension.to_ascii_lowercase();
            path.push(if ext.is_empty() {
                name
            } else {
                format!("{name}.{ext}")
            });
        } else {
            path.push(name);
        }
    }
    Ok(path)
}

/// Fills in fields and drops `[...]` sections with an empty field.
fn expand(pattern: &str, fields: &Fields) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('[') {
            let end = after.find(']').ok_or("a “[” has no matching “]”")?;
            let (section, empty) = fill(&after[..end], fields)?;
            if !empty {
                out.push_str(&section);
            }
            rest = &after[end + 1..];
        } else {
            let end = rest.find('[').unwrap_or(rest.len());
            out.push_str(&fill(&rest[..end], fields)?.0);
            rest = &rest[end..];
        }
    }
    Ok(out)
}

/// Replaces `%...%` tokens; also says whether any token was empty.
fn fill(text: &str, fields: &Fields) -> Result<(String, bool), String> {
    let mut out = String::new();
    let mut empty = false;
    let mut pieces = text.split('%');
    out.push_str(pieces.next().unwrap_or_default());
    let mut token = true;
    for piece in pieces {
        if token {
            let value = token_value(piece, fields);
            empty |= value.is_empty();
            out.push_str(&value);
        } else {
            out.push_str(piece);
        }
        token = !token;
    }
    if !token {
        return Err("a “%” has no matching “%”".into());
    }
    Ok((out, empty))
}

fn token_value(token: &str, fields: &Fields) -> String {
    let (name, width) = match token.rsplit_once(':') {
        Some((n, w)) if w.chars().all(|c| c.is_ascii_digit()) && !w.is_empty() => {
            (n, w.parse::<usize>().ok())
        }
        _ => (token, None),
    };
    let first = |f: &str| match fields.get(f).map(Vec::as_slice).unwrap_or_default() {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} & {last}", init.join(", ")),
    };
    let value = match name {
        "year" => first("date").chars().take(4).collect(),
        f => first(f),
    };
    match width {
        Some(w) if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) => {
            format!("{value:0>w$}")
        }
        Some(w) => value.chars().take(w).collect(),
        None => value,
    }
}

/// A name every filesystem accepts.
pub(crate) fn sanitise(name: &str) -> String {
    let mut s: String = name
        .chars()
        .filter_map(|c| match c {
            '/' | '\\' | ':' | '|' => Some('-'),
            '"' => Some('\''),
            '*' | '<' | '>' => Some('_'),
            '?' => None,
            c if c.is_whitespace() => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect();
    // Collapse runs of spaces; no leading or trailing spaces, no trailing dots (Windows, FAT).
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = s.trim_end_matches(['.', ' ']).trim_start();
    let mut s = trimmed.to_owned();
    // Names Windows reserves.
    let upper = s.to_ascii_uppercase();
    let stem = upper.split('.').next().unwrap_or_default();
    if matches!(stem, "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
    {
        s.insert(0, '_');
    }
    if s.len() > MAX_NAME {
        let mut end = MAX_NAME;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s = s.trim_end_matches(['.', ' ']).to_owned();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(pairs: &[(&str, &str)]) -> Fields {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), vec![(*v).to_owned()]))
            .collect()
    }

    #[test]
    fn patterns_fill_pad_and_drop_empty_sections() {
        let f = fields(&[
            ("albumartist", "Miles Davis"),
            ("album", "Kind of Blue"),
            ("date", "1959-08-17"),
            ("tracknumber", "3"),
            ("title", "Blue in Green"),
        ]);
        let pattern = "%albumartist%/[%year% - ]%album%/[%discnumber%-]%tracknumber:2% %title%";
        assert_eq!(
            relative_path(pattern, &f, "FLAC").unwrap(),
            PathBuf::from("Miles Davis/1959 - Kind of Blue/03 Blue in Green.flac")
        );
        let mut two_discs = f.clone();
        two_discs.insert("discnumber".into(), vec!["2".into()]);
        assert_eq!(
            relative_path(pattern, &two_discs, "flac").unwrap(),
            PathBuf::from("Miles Davis/1959 - Kind of Blue/2-03 Blue in Green.flac")
        );
        assert_eq!(
            relative_path("%album:4%", &f, "flac").unwrap(),
            PathBuf::from("Kind.flac")
        );
        let mut duo = f.clone();
        duo.insert("artist".into(), vec!["A".into(), "B".into(), "C".into()]);
        assert_eq!(
            relative_path("%artist%", &duo, "flac").unwrap(),
            PathBuf::from("A, B & C.flac")
        );
    }

    #[test]
    fn names_are_safe_everywhere() {
        assert_eq!(
            sanitise("AC/DC: Live? \"Best\" <of> *"),
            "AC-DC- Live 'Best' _of_ _"
        );
        assert_eq!(sanitise("  Trailing dots...  "), "Trailing dots");
        assert_eq!(sanitise("con"), "_con");
        assert_eq!(sanitise("COM1.txt"), "_COM1.txt");
        assert_eq!(sanitise("Tab\there"), "Tab here");
        let long = "あ".repeat(100);
        let cut = sanitise(&long);
        assert!(cut.len() <= MAX_NAME && cut.chars().all(|c| c == 'あ'));
    }

    #[test]
    fn missing_names_and_bad_patterns_are_errors() {
        let f = fields(&[("title", "Song")]);
        assert!(relative_path("%albumartist%/%title%", &f, "mp3").is_err());
        assert!(relative_path("%title", &f, "mp3").is_err());
        assert!(relative_path("[%title%", &f, "mp3").is_err());
        assert!(relative_path("", &f, "mp3").is_err());
    }
}
