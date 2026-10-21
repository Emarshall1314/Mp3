//! Vorbis comments (FLAC, Ogg Vorbis, Opus) → canonical fields.

use lofty::ogg::OggPictureStorage;
use lofty::ogg::tag::VorbisComments;

use crate::fields::{self, VORBIS, VORBIS_HIDDEN};
use crate::{Fields, FieldsBuilder, Picture, normalise_rating, split_performer};

pub(crate) fn read(vc: &VorbisComments) -> (Fields, Vec<Picture>) {
    let mut b = FieldsBuilder::default();
    for (key, value) in vc.items() {
        apply(&mut b, key, value, VORBIS);
    }
    let pictures = vc.pictures().iter().map(|(p, _)| picture(p)).collect();
    (b.finish(), pictures)
}

/// Shared by Vorbis comments and APEv2, which use the same key vocabulary.
pub(crate) fn apply(b: &mut FieldsBuilder, key: &str, value: &str, table: &[(&str, &'static str)]) {
    let upper = key.to_ascii_uppercase();
    match upper.as_str() {
        "TRACKNUMBER" => b.push_number_pair("tracknumber", "tracktotal", value),
        "DISCNUMBER" => b.push_number_pair("discnumber", "disctotal", value),
        "PERFORMER" => match split_performer(value) {
            (name, Some(role)) => b.push(&format!("performer:{role}"), name),
            (name, None) => b.push("performer", name),
        },
        "FMPS_RATING" | "RATING" => {
            if let Some(r) = normalise_rating(value) {
                b.push("rating", &r);
            }
        }
        "FMPS_PLAYCOUNT" | "PLAYCOUNT" => b.push("playcount", value.trim()),
        _ if VORBIS_HIDDEN.contains(&upper.as_str()) => {}
        _ => match fields::lookup(table, key) {
            Some(field) => {
                // TRACKTOTAL after "3/12" in TRACKNUMBER: the explicit total wins.
                if matches!(field, "tracktotal" | "disctotal") {
                    b.fields.remove(field);
                }
                b.push(field, value);
            }
            None => b.push_custom(key, value),
        },
    }
}

pub(crate) fn picture(p: &lofty::picture::Picture) -> Picture {
    Picture {
        picture_type: p.pic_type().as_u8(),
        mime: p
            .mime_type()
            .map(|m| m.as_str().to_owned())
            .unwrap_or_default(),
        description: p.description().unwrap_or_default().to_owned(),
        data: p.data().to_vec(),
    }
}
