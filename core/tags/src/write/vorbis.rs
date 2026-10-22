//! Edits to Vorbis comments (FLAC, Ogg Vorbis, Opus).

use lofty::flac::FlacFile;
use lofty::ogg::OggPictureStorage;
use lofty::ogg::tag::VorbisComments;

use super::{Plan, lofty_picture};
use crate::fields::VORBIS;
use crate::{Error, Picture, Result, split_performer};

pub(crate) fn apply(vc: &mut VorbisComments, plan: &Plan) -> Result<()> {
    for field in plan.changes.keys() {
        remove(vc, field);
    }
    for (field, values) in &plan.changes {
        let key = key_for(field)?;
        for value in values {
            let value = match field.as_str() {
                "rating" => fmps_rating(value),
                f if f.starts_with("performer:") => format!("{value} ({})", &f[10..]),
                _ => value.clone(),
            };
            vc.push(key.clone(), value);
        }
    }
    Ok(())
}

/// The key a field is written under.
pub(crate) fn key_for(field: &str) -> Result<String> {
    let key = match field {
        "rating" => "FMPS_RATING".to_owned(),
        "playcount" => "FMPS_PLAYCOUNT".to_owned(),
        "performer" => "PERFORMER".to_owned(),
        f if f.starts_with("performer:") => "PERFORMER".to_owned(),
        f if f.starts_with("custom:") => f["custom:".len()..].to_owned(),
        f => VORBIS
            .iter()
            .find(|(_, mapped)| *mapped == f)
            .map(|(k, _)| (*k).to_owned())
            .ok_or_else(|| Error::Unwritable(format!("{f} has no Vorbis comment key")))?,
    };
    // Keys are ASCII 0x20 to 0x7D, without '='.
    if key.is_empty() || !key.bytes().all(|b| (0x20..=0x7d).contains(&b) && b != b'=') {
        return Err(Error::Unwritable(format!(
            "{key} isn't a valid tag name here"
        )));
    }
    Ok(key)
}

/// Removes every comment the reader would turn into `field`.
fn remove(vc: &mut VorbisComments, field: &str) {
    if field == "performer" || field.starts_with("performer:") {
        let role = field.strip_prefix("performer:");
        retain(vc, |k, v| {
            !(k.eq_ignore_ascii_case("PERFORMER") && split_performer(v).1 == role)
        });
        return;
    }
    let keys: Vec<String> = match field {
        "rating" => vec!["FMPS_RATING".into(), "RATING".into()],
        "playcount" => vec!["FMPS_PLAYCOUNT".into(), "PLAYCOUNT".into()],
        f if f.starts_with("custom:") => vec![f["custom:".len()..].to_owned()],
        f => VORBIS
            .iter()
            .filter(|(_, mapped)| *mapped == f)
            .map(|(k, _)| (*k).to_owned())
            .collect(),
    };
    retain(vc, |k, _| {
        !keys.iter().any(|key| key.eq_ignore_ascii_case(k))
    });
}

/// Keeps the comments `keep` accepts, in their order. (lofty's own `remove` reorders the rest,
/// which changes the order of other fields' values.)
fn retain(vc: &mut VorbisComments, mut keep: impl FnMut(&str, &str) -> bool) {
    let items: Vec<(String, String)> = vc.take_items().collect();
    for (k, v) in items {
        if keep(&k, &v) {
            vc.push(k, v);
        }
    }
}

/// FMPS ratings run from 0 to 1.
pub(crate) fn fmps_rating(value: &str) -> String {
    let v: f64 = value.trim().parse().unwrap_or(0.0);
    let s = format!("{:.2}", v / 100.0);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// FLAC keeps pictures in metadata blocks of their own (and some taggers in the comments).
pub(crate) fn replace_flac_pictures(file: &mut FlacFile, pictures: &[Picture]) -> Result<()> {
    file.remove_pictures();
    if let Some(vc) = file.vorbis_comments_mut() {
        vc.remove_pictures();
    }
    for p in pictures {
        file.insert_picture(lofty_picture(p), None)
            .map_err(|e| Error::Unwritable(format!("picture: {e}")))?;
    }
    Ok(())
}

/// Ogg files keep pictures in the comments (METADATA_BLOCK_PICTURE).
pub(crate) fn replace_pictures(vc: &mut VorbisComments, pictures: &[Picture]) -> Result<()> {
    vc.remove_pictures();
    for p in pictures {
        vc.insert_picture(lofty_picture(p), None)
            .map_err(|e| Error::Unwritable(format!("picture: {e}")))?;
    }
    Ok(())
}
