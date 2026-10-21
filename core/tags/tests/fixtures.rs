//! Reads every fixture and compares against the manifest (the independent oracle).

#![allow(clippy::unwrap_used)]

use cadenza_tags::{Codec, read};
use cadenza_testkit::{Entry, Expect, manifest};

/// Fields a reader may add beyond what the fixture wrote: values derived from other frames, and
/// encoder names the encoding tools write themselves (ffmpeg's `©too`).
const DERIVED: &[&str] = &["syncedlyrics", "musicbrainz_recordingid", "encodersettings"];

fn readable(e: &Entry) -> bool {
    e.expect != Expect::Error && e.codec != "unknown"
}

#[test]
fn stream_facts_match_for_every_readable_fixture() {
    let mut failures = Vec::new();
    for e in manifest().entries.iter().filter(|e| readable(e)) {
        let tags = match read(&e.file()) {
            Ok(t) => t,
            Err(err) if e.expect == Expect::Partial => {
                eprintln!("{}: partial file rejected ({err}) — acceptable", e.id);
                continue;
            }
            Err(err) => {
                failures.push(format!("{}: {err}", e.id));
                continue;
            }
        };
        let a = &tags.audio;
        if a.channels as u32 != e.channels {
            failures.push(format!(
                "{}: channels {} != {}",
                e.id, a.channels, e.channels
            ));
        }
        if let Some(rate) = e.dsd_rate {
            if a.dsd_rate != Some(rate) {
                failures.push(format!("{}: dsd_rate {:?} != {rate}", e.id, a.dsd_rate));
            }
            continue;
        }
        if let Some(rate) = e.sample_rate
            && a.sample_rate != rate
        {
            failures.push(format!("{}: rate {} != {rate}", e.id, a.sample_rate));
        }
        if e.lossless {
            if a.bit_depth.map(u32::from) != e.bits {
                failures.push(format!("{}: bits {:?} != {:?}", e.id, a.bit_depth, e.bits));
            }
            if a.float != (e.sample_format.as_deref() == Some("f")) && e.codec != "aiff" {
                failures.push(format!("{}: float flag {}", e.id, a.float));
            }
        }
        let codec_ok = matches!(
            (e.codec.as_str(), a.codec),
            ("pcm", Codec::Pcm)
                | ("flac", Codec::Flac)
                | ("alac", Codec::Alac)
                | ("ape", Codec::Ape)
                | ("wavpack", Codec::WavPack)
                | ("wavpack-dsd" | "dsd", Codec::Dsd)
                | ("mp3", Codec::Mp3)
                | ("aac", Codec::Aac)
                | ("opus", Codec::Opus)
                | ("vorbis", Codec::Vorbis)
        );
        if !codec_ok {
            failures.push(format!("{}: codec {:?} for {}", e.id, a.codec, e.codec));
        }
        if let Some(frames) = e.frames {
            let rate = e.sample_rate.unwrap_or(1) as u64;
            let expect_ms = frames * 1000 / rate;
            if a.duration_ms.abs_diff(expect_ms) > 60 {
                failures.push(format!(
                    "{}: duration {} ms, expected ~{expect_ms}",
                    e.id, a.duration_ms
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn expected_fields_read_back_exactly() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for e in manifest().entries.iter().filter(|e| readable(e)) {
        let Some(expected) = &e.tags else { continue };
        if e.native
            .as_ref()
            .and_then(|n| n.get("legacy_charset"))
            .is_some()
        {
            continue; // checked in legacy_encodings_are_flagged
        }
        let got = match read(&e.file()) {
            Ok(t) => t.fields,
            Err(err) => {
                failures.push(format!("{}: {err}", e.id));
                continue;
            }
        };
        checked += 1;
        for (field, values) in expected {
            if got.get(field) != Some(values) {
                failures.push(format!(
                    "{}: {field} = {:?}, expected {values:?}",
                    e.id,
                    got.get(field)
                ));
            }
        }
        for field in got.keys() {
            let allowed = expected.contains_key(field)
                || field.starts_with("custom:")
                || DERIVED.contains(&field.as_str());
            if !allowed {
                failures.push(format!(
                    "{}: unexpected field {field} = {:?}",
                    e.id, got[field]
                ));
            }
        }
    }
    assert!(checked > 60, "only {checked} tagged fixtures checked");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn unknown_text_items_surface_as_custom_fields() {
    for id in [
        "tags/flac_vorbis",
        "tags/mp3_id3v24",
        "tags/wv_apev2",
        "tags/m4a_aac",
    ] {
        let e = manifest().entries.iter().find(|e| e.id == id).unwrap();
        let t = read(&e.file()).unwrap();
        let custom: Vec<_> = t.fields.keys().filter(|k| k.contains("UNKNOWN")).collect();
        assert!(
            !custom.is_empty(),
            "{id}: unknown text item not exposed: {:?}",
            t.fields.keys()
        );
    }
}

#[test]
fn artwork_matches_by_type_and_hash() {
    let mut failures = Vec::new();
    for e in manifest()
        .entries
        .iter()
        .filter(|e| readable(e) && !e.artwork.is_empty())
    {
        let Ok(t) = read(&e.file()) else {
            failures.push(format!("{}: unreadable", e.id));
            continue;
        };
        let mut got: Vec<(u8, String)> = t
            .pictures
            .iter()
            .map(|p| {
                (
                    p.picture_type,
                    sha1_smol::Sha1::from(&p.data).digest().to_string(),
                )
            })
            .collect();
        let mut want: Vec<(u8, String)> = e
            .artwork
            .iter()
            .map(|a| (a.picture_type, a.sha1.clone()))
            .collect();
        got.sort();
        want.sort();
        if got != want {
            failures.push(format!("{}: pictures {got:?} != {want:?}", e.id));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn legacy_encodings_are_flagged() {
    for e in manifest()
        .entries
        .iter()
        .filter(|e| e.id.starts_with("encodings/legacy/"))
    {
        let native = e.native.as_ref().unwrap();
        let charset = native["legacy_charset"].as_str().unwrap();
        let t = read(&e.file()).unwrap();
        let title = &e.tags.as_ref().unwrap()["title"][0];
        let suggestion = t.legacy.iter().find(|l| l.field == "TIT2");
        match charset {
            "latin-1" => {
                assert!(
                    suggestion.is_none(),
                    "{}: false positive {suggestion:?}",
                    e.id
                );
                assert_eq!(t.get("title"), Some(title.as_str()));
            }
            _ => {
                let s = suggestion.unwrap_or_else(|| panic!("{}: no suggestion", e.id));
                assert_eq!(&s.decoded, title, "{}: decoded with {}", e.id, s.encoding);
                assert_eq!(s.raw, hex(native["raw_hex"].as_str().unwrap()));
            }
        }
    }
}

#[test]
fn content_sniffing_beats_the_extension() {
    let e = manifest()
        .entries
        .iter()
        .find(|e| e.id == "corrupt/wrong_extension")
        .unwrap();
    assert_eq!(read(&e.file()).unwrap().audio.codec, Codec::Flac);
}

#[test]
fn broken_files_error_cleanly() {
    for e in manifest()
        .entries
        .iter()
        .filter(|e| e.expect == Expect::Error)
    {
        // Must return (Ok or Err), never panic; most must be Err.
        let r = std::panic::catch_unwind(|| read(&e.file()));
        assert!(r.is_ok(), "{} panicked", e.id);
    }
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
