//! Lyrics: a sidecar `.lrc` next to the file, else embedded synced lyrics, else embedded plain
//! lyrics (which are often LRC too). LRC timestamps make lyrics synced.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    /// Start time in ms; `None` for unsynced lyrics.
    pub time_ms: Option<u64>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub synced: bool,
    pub lines: Vec<LyricLine>,
    /// Where they came from: "file" (a sidecar .lrc) or "embedded".
    pub source: String,
}

/// Parses `[mm:ss.xx]` timestamps (several per line allowed), honours `[offset:±ms]`, drops
/// metadata tags (`[ar:…]`) and enhanced word stamps (`<mm:ss.xx>`). Text without timestamps comes
/// back unsynced, line by line.
pub fn parse(text: &str, source: &str) -> Option<Lyrics> {
    let mut timed: Vec<(i64, String)> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    let mut offset = 0i64;
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(body) = rest.strip_prefix('[') {
            let Some(end) = body.find(']') else { break };
            let tag = &body[..end];
            if let Some(ms) = timestamp(tag) {
                stamps.push(ms);
            } else if let Some(v) = tag.strip_prefix("offset:") {
                offset = v.trim().parse().unwrap_or(0);
            } else if !tag.contains(':') {
                break;
            }
            rest = body[end + 1..].trim_start();
        }
        let line = strip_word_stamps(rest);
        if stamps.is_empty() {
            if !(raw.trim().starts_with('[') && line.is_empty()) {
                plain.push(line);
            }
        } else {
            timed.extend(stamps.into_iter().map(|t| (t, line.clone())));
        }
    }
    if !timed.is_empty() {
        // LRC offset: positive shows lines earlier.
        timed.sort_by_key(|(t, _)| *t);
        let lines = timed
            .into_iter()
            .map(|(t, text)| LyricLine {
                time_ms: Some((t - offset).max(0) as u64),
                text,
            })
            .collect();
        return Some(Lyrics {
            synced: true,
            lines,
            source: source.into(),
        });
    }
    while plain.last().is_some_and(String::is_empty) {
        plain.pop();
    }
    let first = plain.iter().position(|l| !l.is_empty())?;
    Some(Lyrics {
        synced: false,
        lines: plain[first..]
            .iter()
            .map(|t| LyricLine {
                time_ms: None,
                text: t.clone(),
            })
            .collect(),
        source: source.into(),
    })
}

/// `mm:ss`, `mm:ss.x`, `mm:ss.xx`, `mm:ss.xxx`, `mm:ss:xx` → ms.
fn timestamp(tag: &str) -> Option<i64> {
    let (min, sec) = tag.split_once(':')?;
    let min: i64 = min.trim().parse().ok()?;
    let sec = sec.replacen(':', ".", 1);
    let (whole, frac) = sec.split_once('.').unwrap_or((&sec, ""));
    let whole: i64 = whole.parse().ok()?;
    if !(0..60).contains(&whole) || !frac.chars().all(|c| c.is_ascii_digit()) || frac.len() > 3 {
        return None;
    }
    let frac_ms = if frac.is_empty() {
        0
    } else {
        frac.parse::<i64>().ok()? * 10i64.pow(3 - frac.len() as u32)
    };
    Some(min * 60_000 + whole * 1000 + frac_ms)
}

fn strip_word_stamps(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        match rest[i..].find('>') {
            Some(j) if timestamp(&rest[i + 1..i + j]).is_some() => {
                out.push_str(&rest[..i]);
                rest = &rest[i + j + 1..];
            }
            _ => {
                out.push_str(&rest[..=i]);
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_parses_with_repeats_offsets_and_metadata() {
        let lrc = "[ar:Someone]\n[ti:Song]\n[offset:+500]\n\
                   [00:12.00]First line\n\
                   [00:15.30][01:02.5]Chorus <00:15.80>word\n\
                   [00:20.123]\n";
        let l = parse(lrc, "file").unwrap();
        assert!(l.synced);
        let got: Vec<(Option<u64>, &str)> = l
            .lines
            .iter()
            .map(|l| (l.time_ms, l.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (Some(11_500), "First line"),
                (Some(14_800), "Chorus word"),
                (Some(19_623), ""),
                (Some(62_000), "Chorus word"),
            ]
        );
    }

    #[test]
    fn plain_text_is_unsynced() {
        let l = parse("\nVerse one\nVerse two\n\nVerse three\n\n", "embedded").unwrap();
        assert!(!l.synced);
        assert_eq!(l.lines.len(), 4);
        assert_eq!(l.lines[0].text, "Verse one");
        assert!(parse("  \n\n", "embedded").is_none());
        // Brackets that aren't timestamps stay text.
        assert_eq!(
            parse("[Chorus]\nla la", "embedded").unwrap().lines[0].text,
            "[Chorus]"
        );
    }
}
