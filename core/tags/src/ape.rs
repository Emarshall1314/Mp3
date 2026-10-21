//! APEv2 (WavPack, Monkey's Audio, sometimes MP3) → canonical fields.

use lofty::ape::ApeTag;
use lofty::tag::ItemValue;

use crate::fields::{APE_EXTRA, VORBIS};
use crate::{Fields, FieldsBuilder, Picture};

pub(crate) fn read(tag: &ApeTag) -> (Fields, Vec<Picture>) {
    let mut b = FieldsBuilder::default();
    let mut pictures = Vec::new();
    let table: Vec<(&str, &'static str)> = APE_EXTRA.iter().chain(VORBIS).copied().collect();
    for item in tag {
        let key = item.key();
        match item.value() {
            ItemValue::Text(text) => {
                // APEv2 separates multiple values with NUL.
                for value in text.split('\0').filter(|v| !v.is_empty()) {
                    match key.to_ascii_uppercase().as_str() {
                        "TRACK" => b.push_number_pair("tracknumber", "tracktotal", value),
                        "DISC" => b.push_number_pair("discnumber", "disctotal", value),
                        _ => crate::vorbis::apply(&mut b, key, value, &table),
                    }
                }
            }
            ItemValue::Binary(data) => {
                if let Some(kind) = key.strip_prefix("Cover Art (")
                    && let Some(p) = cover_art(kind, data.as_slice())
                {
                    pictures.push(p);
                }
            }
            // External locators are kept by the writer but are not fields.
            ItemValue::Locator(_) => {}
        }
    }
    (b.finish(), pictures)
}

/// APEv2 cover art: "filename\0<image bytes>" under keys like "Cover Art (Front)".
fn cover_art(kind: &str, data: &[u8]) -> Option<Picture> {
    let split = data.iter().position(|&c| c == 0)?;
    let (name, image) = (&data[..split], &data[split + 1..]);
    let picture_type = match kind.trim_end_matches(')').to_ascii_lowercase().as_str() {
        "front" => 3,
        "back" => 4,
        "artist" => 8,
        "media" => 6,
        _ => 0,
    };
    let name = String::from_utf8_lossy(name);
    let mime = if image.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    Some(Picture {
        picture_type,
        mime: mime.to_owned(),
        description: name.into_owned(),
        data: image.to_vec(),
    })
}
