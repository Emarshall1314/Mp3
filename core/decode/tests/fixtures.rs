//! Decoding every fixture against the manifest: bit-exact PCM, exact lengths, gapless joins,
//! sample-accurate seeks, DSD bit-exactness and graceful failure on corrupt files.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use cadenza_decode::{Bounded, Chunk, Decoder, DsdToPcm, SampleFormat, open};
use cadenza_testkit::{Entry, Expect, manifest};
use md5::{Digest, Md5};

/// Decodes a whole file into the canonical byte layout used by the manifest's `pcm_md5`.
fn canonical(dec: &mut dyn Decoder) -> (Vec<u8>, u64) {
    let spec = dec.spec().clone();
    let mut out = Vec::new();
    let mut frames = 0;
    while let Some(chunk) = dec.next_chunk().unwrap() {
        frames += chunk.frames(spec.channels) as u64;
        match (chunk, spec.format) {
            (Chunk::Int(s), SampleFormat::Int(bits)) => {
                let width = (bits as usize).div_ceil(8);
                for v in s {
                    out.extend_from_slice(&v.to_le_bytes()[..width]);
                }
            }
            (Chunk::Float(s), _) => s
                .iter()
                .for_each(|v| out.extend_from_slice(&v.to_le_bytes())),
            (Chunk::Dsd(s), _) => out.extend_from_slice(s),
            (c, f) => panic!("chunk {c:?} for format {f:?}"),
        }
    }
    (out, frames)
}

fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn ok_entries() -> impl Iterator<Item = &'static Entry> {
    manifest()
        .entries
        .iter()
        .filter(|e| e.expect == Expect::Ok && e.codec != "unknown")
}

#[test]
fn lossless_pcm_is_bit_exact() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for e in ok_entries().filter(|e| e.pcm_md5.is_some()) {
        if e.codec == "flac" && e.bits == Some(32) {
            // Known gap (PROGRESS.md): rejected cleanly instead of decoded wrongly.
            assert!(
                matches!(open(&e.file()), Err(cadenza_decode::Error::Unsupported(_))),
                "{}",
                e.id
            );
            continue;
        }
        let mut dec = match open(&e.file()) {
            Ok(d) => d,
            Err(err) => {
                failures.push(format!("{}: open: {err}", e.id));
                continue;
            }
        };
        let (bytes, frames) = canonical(dec.as_mut());
        checked += 1;
        if Some(frames) != e.frames {
            failures.push(format!(
                "{}: {frames} frames, expected {:?}",
                e.id, e.frames
            ));
        } else if Some(md5_hex(&bytes)) != e.pcm_md5 {
            failures.push(format!("{}: PCM differs ({:?})", e.id, dec.spec().format));
        }
    }
    assert!(checked > 70, "only {checked} lossless fixtures");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn dsd_is_bit_exact() {
    let mut failures = Vec::new();
    for e in ok_entries().filter(|e| e.dsd_md5.is_some()) {
        let mut dec = open(&e.file()).unwrap();
        assert_eq!(dec.spec().sample_rate, e.dsd_rate.unwrap(), "{}", e.id);
        let (bytes, frames) = canonical(dec.as_mut());
        if Some(frames) != e.frames || Some(md5_hex(&bytes)) != e.dsd_md5 {
            failures.push(format!(
                "{}: {frames} frames (want {:?}), md5 match {}",
                e.id,
                e.frames,
                Some(md5_hex(&bytes)) == e.dsd_md5
            ));
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
fn lossy_streams_have_exact_lengths() {
    let mut failures = Vec::new();
    for e in ok_entries().filter(|e| !e.lossless && e.frames.is_some()) {
        let mut dec = open(&e.file()).unwrap_or_else(|err| panic!("{}: {err}", e.id));
        let (_, frames) = canonical(dec.as_mut());
        if Some(frames) != e.frames {
            failures.push(format!(
                "{} ({}): {frames} frames, expected {}",
                e.id,
                e.codec,
                e.frames.unwrap()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Interleaved f32 samples of a whole decoder.
fn floats(dec: &mut dyn Decoder) -> Vec<f32> {
    let spec = dec.spec().clone();
    let mut out = Vec::new();
    while let Some(chunk) = dec.next_chunk().unwrap() {
        match chunk {
            Chunk::Int(s) => {
                let SampleFormat::Int(bits) = spec.format else {
                    unreachable!()
                };
                let scale = (1u64 << (bits - 1)) as f32;
                out.extend(s.iter().map(|&v| v as f32 / scale));
            }
            Chunk::Float(s) => out.extend_from_slice(s),
            Chunk::Dsd(_) => panic!("DSD in floats()"),
        }
    }
    out
}

#[test]
fn gapless_sets_join_without_gaps_or_clicks() {
    let mut sets: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
    for e in manifest().entries.iter().filter(|e| e.gapless.is_some()) {
        sets.entry(e.gapless.as_ref().unwrap().set.as_str())
            .or_default()
            .push(e);
    }
    let mut failures = Vec::new();
    for (name, mut parts) in sets {
        parts.sort_by_key(|e| e.gapless.as_ref().unwrap().index);
        let g = parts[0].gapless.as_ref().unwrap();
        if parts[0].lossless {
            let mut all = Vec::new();
            for p in &parts {
                all.extend(canonical(open(&p.file()).unwrap().as_mut()).0);
            }
            if md5_hex(&all) != g.source_md5 {
                failures.push(format!(
                    "{name}: joined lossless decode differs from source"
                ));
            }
            continue;
        }
        // Lossy: compare the joined decode with the known source signal around each boundary.
        let rate = parts[0].sample_rate.unwrap() as f64;
        let mut joined = Vec::new();
        for p in &parts {
            joined.extend(floats(open(&p.file()).unwrap().as_mut()));
        }
        let frames = joined.len() / 2;
        if frames as u64 != g.source_frames {
            failures.push(format!(
                "{name}: joined length {frames} != {}",
                g.source_frames
            ));
            continue;
        }
        let amp = 10f64.powf(-6.0 / 20.0);
        let expected = |n: usize, c: usize| {
            amp * (2.0 * std::f64::consts::PI * [441.7, 662.3][c] * n as f64 / rate).sin()
        };
        let rms_err = |from: usize, to: usize| {
            let mut sum = 0.0;
            for n in from..to {
                for c in 0..2 {
                    sum += (joined[n * 2 + c] as f64 - expected(n, c)).powi(2);
                }
            }
            (sum / ((to - from) * 2) as f64).sqrt()
        };
        // Each part must sit exactly where the source had it: the error against the known signal
        // must be smallest with zero shift. (Absolute error near the cuts is the encoder's
        // business: every part starts abruptly, and AAC in particular smears such onsets.)
        for p in &parts[1..] {
            let b = p.gapless.as_ref().unwrap().start_frame as usize;
            let shifted = |k: i64| {
                let mut sum = 0.0;
                for n in b..b + 2048 {
                    let m = (n as i64 + k) as usize;
                    sum += (joined[m * 2] as f64 - expected(n, 0)).powi(2);
                }
                sum
            };
            let errs: Vec<f64> = (-3..=3).map(shifted).collect();
            let best = errs
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.total_cmp(b.1))
                .map(|(i, _)| i as i64 - 3);
            if best != Some(0) {
                failures.push(format!(
                    "{name}: part starting at {b} is offset by {best:?} frames"
                ));
            }
        }
        let mid = rms_err(frames / 2 - 4096, frames / 2 + 4096);
        if mid > 0.01 {
            failures.push(format!("{name}: mid-track error {mid:.5} (decode broken?)"));
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
fn seeks_land_on_the_exact_sample() {
    let ids = [
        "formats/flac_s24_96000",
        "formats/wav_s16_44100",
        "formats/alac_s24_96000",
        "formats/aiff_s24_96000",
        "formats/wavpack_s24_96000",
        "formats/wavpack_f32_192000",
        "formats/ape_s16_44100",
        "formats/wav_f32_44100",
    ];
    for id in ids {
        let e = manifest().entries.iter().find(|e| e.id == id).unwrap();
        let full = canonical(open(&e.file()).unwrap().as_mut()).0;
        let bytes_per_frame = full.len() / e.frames.unwrap() as usize;
        for target in [0u64, 1, 4095, 12_345, e.frames.unwrap() / 2] {
            let mut dec = open(&e.file()).unwrap();
            let reached = dec.seek(target).unwrap();
            assert_eq!(reached, target, "{id}: seek reached {reached}");
            let rest = canonical(dec.as_mut()).0;
            assert!(
                rest == full[target as usize * bytes_per_frame..],
                "{id}: samples after seek to {target} differ"
            );
        }
    }
}

#[test]
fn lossy_seeks_are_accurate() {
    for id in [
        "formats/mp3-cbr_s16_44100",
        "formats/vorbis_s16_44100",
        "formats/opus_s16_48000",
        "formats/aac_s16_44100",
    ] {
        let e = manifest().entries.iter().find(|e| e.id == id).unwrap();
        let full = floats(open(&e.file()).unwrap().as_mut());
        let target = 20_000usize;
        let mut dec = open(&e.file()).unwrap();
        dec.seek(target as u64).unwrap();
        let rest = floats(dec.as_mut());
        assert_eq!(
            rest.len() / 2,
            full.len() / 2 - target,
            "{id}: length after seek"
        );
        // After a seek the decoder state is rebuilt from pre-roll, so lossy output may differ
        // slightly from a straight-through decode; it must not be shifted, and must be close.
        let rms = |k: i64| {
            let mut sum = 0.0f64;
            for n in 2048..6144usize {
                let a = rest[n * 2] as f64;
                let b = full[((target + n) as i64 + k) as usize * 2] as f64;
                sum += (a - b).powi(2);
            }
            (sum / 4096.0).sqrt()
        };
        let errs: Vec<f64> = (-2..=2).map(rms).collect();
        assert!(
            errs[2] <= errs.iter().copied().fold(f64::MAX, f64::min),
            "{id}: seek lands off by a few frames: {errs:?}"
        );
        assert!(
            errs[2] < 0.005,
            "{id}: RMS difference after seek {:.5}",
            errs[2]
        );
    }
}

#[test]
fn dsd_to_pcm_keeps_level_length_and_tone() {
    for id in [
        "formats/dsf_dsd64",
        "formats/dff_dsd128",
        "formats/dsf_dsd256",
    ] {
        let e = manifest().entries.iter().find(|e| e.id == id).unwrap();
        let mut pcm = DsdToPcm::new(open(&e.file()).unwrap()).unwrap();
        let rate = pcm.spec().sample_rate as f64;
        assert_eq!(rate, 176_400.0);
        let samples = floats(&mut pcm);
        let frames = samples.len() / 2;
        assert_eq!(
            frames as u64,
            e.frames.unwrap() / (e.dsd_rate.unwrap() as u64 / 176_400),
            "{id}: length"
        );
        // Amplitude of the 997 Hz tone on the left channel (fixture: 0.5 modulation).
        let (from, to) = (frames / 4, frames * 3 / 4);
        let w = 2.0 * std::f64::consts::PI * 997.0 / rate;
        let (mut re, mut im) = (0.0, 0.0);
        for n in from..to {
            re += samples[n * 2] as f64 * (w * n as f64).cos();
            im += samples[n * 2] as f64 * (w * n as f64).sin();
        }
        let amplitude = 2.0 * (re * re + im * im).sqrt() / (to - from) as f64;
        assert!(
            (amplitude - 0.5).abs() < 0.01,
            "{id}: 997 Hz amplitude {amplitude:.4}"
        );
        let peak = samples.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(
            peak < 0.7,
            "{id}: peak {peak} (ultrasonic noise not filtered?)"
        );
    }
}

#[test]
fn cue_ranges_decode_exactly() {
    let e = manifest()
        .entries
        .iter()
        .find(|e| e.id == "cue/sidecar_utf8/album")
        .unwrap();
    let full = canonical(open(&e.file()).unwrap().as_mut()).0;
    let tracks = e.cue.as_ref().unwrap()["tracks"]
        .as_array()
        .unwrap()
        .clone();
    let start = tracks[1]["start_frame"].as_u64().unwrap();
    let end = tracks[2]["start_frame"].as_u64().unwrap();
    let mut b = Bounded::new(open(&e.file()).unwrap(), start, Some(end)).unwrap();
    let (bytes, frames) = canonical(&mut b);
    assert_eq!(frames, end - start);
    assert!(bytes == full[start as usize * 4..end as usize * 4]);
}

#[test]
fn corrupt_files_fail_without_panicking() {
    for e in manifest().entries.iter().filter(|e| e.group == "corrupt") {
        let result = std::panic::catch_unwind(|| {
            let mut dec = open(&e.file())?;
            let channels = dec.spec().channels;
            let mut frames = 0;
            loop {
                match dec.next_chunk() {
                    Ok(Some(c)) => frames += c.frames(channels),
                    Ok(None) => return Ok(frames),
                    Err(err) if frames > 0 => {
                        eprintln!("{}: stopped after {frames} frames: {err}", e.id);
                        return Ok(frames);
                    }
                    Err(err) => return Err(err),
                }
            }
        });
        let Ok(outcome) = result else {
            panic!("{} panicked", e.id)
        };
        if e.expect == Expect::Partial {
            assert!(
                outcome.as_ref().map(|f| *f > 0).unwrap_or(false),
                "{}: expected some audio, got {outcome:?}",
                e.id
            );
        }
    }
}
