//! Detection of legacy 8-bit text stored as Latin-1.
//!
//! Old taggers wrote Shift-JIS, CP1251 and similar bytes into frames flagged Latin-1. Latin-1
//! decoding is lossless (byte n → U+00nn), so the original bytes can be recovered from the string
//! and run through a charset detector. The text is shown as stored; the suggestion feeds the
//! encoding fixer, which only rewrites when the owner accepts.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LegacyText {
    /// Field or frame the text came from (e.g. `TIT2`, `title`).
    pub field: String,
    #[serde(skip)]
    pub raw: Vec<u8>,
    /// WHATWG name of the likely encoding, e.g. "Shift_JIS", "windows-1251".
    pub encoding: &'static str,
    /// The text decoded with that encoding.
    pub decoded: String,
}

pub(crate) fn detect(field: &str, value: &str) -> Option<LegacyText> {
    if value.chars().all(|c| (c as u32) < 0x80) || value.chars().any(|c| (c as u32) > 0xFF) {
        return None; // plain ASCII, or not Latin-1-decoded bytes at all
    }
    let raw: Vec<u8> = value.chars().map(|c| c as u32 as u8).collect();
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(&raw, true);
    let encoding = detector.guess(None, chardetng::Utf8Detection::Deny);
    if encoding == encoding_rs::WINDOWS_1252 {
        return None; // genuinely Western European text
    }
    let (decoded, had_errors) = encoding.decode_without_bom_handling(&raw);
    if had_errors || decoded == value {
        return None;
    }
    let decoded = decoded.into_owned();
    Some(LegacyText {
        field: field.to_owned(),
        raw,
        encoding: encoding.name(),
        decoded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_latin1(bytes: &[u8]) -> String {
        bytes.iter().map(|&b| b as char).collect()
    }

    #[test]
    fn plain_and_western_text_is_left_alone() {
        assert_eq!(detect("t", "Hello"), None);
        assert_eq!(detect("t", "Björk - Jóga"), None);
        assert_eq!(detect("t", "東京"), None); // already proper Unicode
    }

    #[test]
    fn cyrillic_cp1251_is_detected() {
        let (bytes, _, _) = encoding_rs::WINDOWS_1251.encode("Кино — Группа крови");
        let found = detect("TIT2", &as_latin1(&bytes)).expect("detected");
        assert_eq!(found.encoding, "windows-1251");
        assert_eq!(found.decoded, "Кино — Группа крови");
    }
}
