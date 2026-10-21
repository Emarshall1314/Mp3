//! CUE sheets: single-file albums split into tracks.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CueSheet {
    pub file: Option<String>,
    pub title: Option<String>,
    pub performer: Option<String>,
    pub date: Option<String>,
    pub genre: Option<String>,
    pub tracks: Vec<CueTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CueTrack {
    pub number: u32,
    pub title: Option<String>,
    pub performer: Option<String>,
    pub isrc: Option<String>,
    /// INDEX 01 in CD frames (1/75 s).
    pub start_cd_frames: u64,
}

/// Decodes a sheet's bytes: UTF-8 (with or without BOM), else the most likely legacy encoding.
pub(crate) fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_owned();
    }
    let mut det = chardetng_shim::detector();
    det.feed(bytes, true);
    let enc = det.guess(None, chardetng_shim::utf8_deny());
    enc.decode_without_bom_handling(bytes).0.into_owned()
}

/// chardetng is reached through cadenza-tags' dependency tree; keep the call sites tidy.
mod chardetng_shim {
    pub fn detector() -> chardetng::EncodingDetector {
        chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny)
    }
    pub fn utf8_deny() -> chardetng::Utf8Detection {
        chardetng::Utf8Detection::Deny
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(s)
        .to_owned()
}

fn parse_time(s: &str) -> Option<u64> {
    let mut parts = s.trim().split(':');
    let m: u64 = parts.next()?.parse().ok()?;
    let sec: u64 = parts.next()?.parse().ok()?;
    let f: u64 = parts.next()?.parse().ok()?;
    Some((m * 60 + sec) * 75 + f)
}

pub(crate) fn parse(text: &str) -> CueSheet {
    let mut sheet = CueSheet::default();
    let mut current: Option<CueTrack> = None;
    for line in text.lines() {
        let line = line.trim();
        let (cmd, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match cmd.to_ascii_uppercase().as_str() {
            "FILE" if sheet.file.is_none() => {
                // FILE "name" TYPE — the name may contain spaces.
                let name = match rest.trim().strip_prefix('"') {
                    Some(r) => r.split('"').next().unwrap_or_default().to_owned(),
                    None => rest
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                };
                sheet.file = Some(name);
            }
            "TRACK" => {
                if let Some(t) = current.take() {
                    sheet.tracks.push(t);
                }
                let number = rest
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                current = Some(CueTrack {
                    number,
                    ..Default::default()
                });
            }
            "TITLE" => match current.as_mut() {
                Some(t) => t.title = Some(unquote(rest)),
                None => sheet.title = Some(unquote(rest)),
            },
            "PERFORMER" => match current.as_mut() {
                Some(t) => t.performer = Some(unquote(rest)),
                None => sheet.performer = Some(unquote(rest)),
            },
            "ISRC" => {
                if let Some(t) = current.as_mut() {
                    t.isrc = Some(unquote(rest));
                }
            }
            "INDEX" => {
                let mut it = rest.split_whitespace();
                if let (Some("01"), Some(time), Some(t)) = (it.next(), it.next(), current.as_mut())
                    && let Some(frames) = parse_time(time)
                {
                    t.start_cd_frames = frames;
                }
            }
            "REM" => {
                let (key, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                match key.to_ascii_uppercase().as_str() {
                    "DATE" => sheet.date = Some(unquote(value)),
                    "GENRE" => sheet.genre = Some(unquote(value)),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if let Some(t) = current {
        sheet.tracks.push(t);
    }
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tracks_pregaps_and_metadata() {
        let sheet = parse(
            "REM DATE 2020\r\nPERFORMER \"A B\"\r\nTITLE \"Album\"\r\nFILE \"my album.flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    TITLE \"One\"\r\n    INDEX 01 00:00:00\r\n  TRACK 02 AUDIO\r\n    TITLE \"Two\"\r\n    INDEX 00 00:01:60\r\n    INDEX 01 00:02:00\r\n",
        );
        assert_eq!(sheet.file.as_deref(), Some("my album.flac"));
        assert_eq!(sheet.date.as_deref(), Some("2020"));
        assert_eq!(sheet.tracks.len(), 2);
        assert_eq!(sheet.tracks[1].start_cd_frames, 150);
        assert_eq!(sheet.tracks[1].title.as_deref(), Some("Two"));
    }
}
