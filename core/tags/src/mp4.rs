//! MP4 `ilst` atoms (AAC, ALAC) → canonical fields.

use lofty::mp4::{AtomData, AtomIdent, Ilst};

use crate::fields::{self, MP4_ATOMS, MP4_FREEFORM};
use crate::{Fields, FieldsBuilder, Picture, normalise_rating, split_performer};

pub(crate) fn read(ilst: &Ilst) -> (Fields, Vec<Picture>) {
    let mut b = FieldsBuilder::default();
    for atom in ilst {
        match atom.ident() {
            AtomIdent::Fourcc(code) => {
                let code_str: String = code.iter().map(|&c| c as char).collect();
                for data in atom.data() {
                    fourcc(&mut b, &code_str, data);
                }
            }
            AtomIdent::Freeform { mean, name } => {
                let values: Vec<String> = atom.data().filter_map(text).collect();
                freeform(&mut b, mean.as_ref(), name.as_ref(), &values);
            }
        }
    }
    let pictures = ilst
        .pictures()
        .map(|ps| {
            ps.map(|p| Picture {
                // MP4 covers carry no type; treat them all as front covers.
                picture_type: 3,
                mime: p
                    .mime_type()
                    .map(|m| m.as_str().to_owned())
                    .unwrap_or_default(),
                description: String::new(),
                data: p.data().to_vec(),
            })
            .collect()
        })
        .unwrap_or_default();
    (b.finish(), pictures)
}

fn text(data: &AtomData) -> Option<String> {
    match data {
        AtomData::UTF8(s) | AtomData::UTF16(s) => Some(s.clone()),
        AtomData::Unknown { data, .. } => String::from_utf8(data.clone()).ok(),
        _ => None,
    }
}

fn fourcc(b: &mut FieldsBuilder, code: &str, data: &AtomData) {
    match (code, data) {
        ("trkn" | "disk", AtomData::Unknown { data, .. }) if data.len() >= 6 => {
            // Big-endian: [reserved u16][number u16][total u16]...
            let n = u16::from_be_bytes([data[2], data[3]]);
            let total = u16::from_be_bytes([data[4], data[5]]);
            let (nf, tf) = if code == "trkn" {
                ("tracknumber", "tracktotal")
            } else {
                ("discnumber", "disctotal")
            };
            if n > 0 {
                b.push(nf, &n.to_string());
            }
            if total > 0 {
                b.push(tf, &total.to_string());
            }
        }
        ("tmpo" | "©mvi" | "©mvc", AtomData::SignedInteger(v)) => {
            let field = match code {
                "tmpo" => "bpm",
                "©mvi" => "movement",
                _ => "movementtotal",
            };
            b.push(field, &v.to_string());
        }
        ("tmpo" | "©mvi" | "©mvc", AtomData::Unknown { data, .. }) => {
            let v = data.iter().fold(0u64, |acc, &x| (acc << 8) | x as u64);
            let field = match code {
                "tmpo" => "bpm",
                "©mvi" => "movement",
                _ => "movementtotal",
            };
            b.push(field, &v.to_string());
        }
        ("cpil", AtomData::Bool(v)) => b.push("compilation", if *v { "1" } else { "0" }),
        ("cpil", AtomData::SignedInteger(v)) => {
            b.push("compilation", if *v != 0 { "1" } else { "0" })
        }
        ("cpil", AtomData::Unknown { data, .. }) => b.push(
            "compilation",
            if data.iter().any(|&x| x != 0) {
                "1"
            } else {
                "0"
            },
        ),
        ("covr", _) => {}
        ("gnre", AtomData::Unknown { data, .. }) if data.len() >= 2 => {
            // Legacy iTunes genre: ID3v1 index + 1.
            let i = u16::from_be_bytes([data[0], data[1]]) as usize;
            if let Some(name) = i.checked_sub(1).and_then(|i| lofty::id3::v1::GENRES.get(i)) {
                b.push("genre", name);
            }
        }
        _ => {
            let Some(value) = text(data) else { return };
            match fields::lookup(MP4_ATOMS, code) {
                Some(field) => b.push(field, &value),
                None => b.push_custom(code, &value),
            }
        }
    }
}

fn freeform(b: &mut FieldsBuilder, mean: &str, name: &str, values: &[String]) {
    // iTunes' own bookkeeping atoms (iTunSMPB, iTunNORM, ...) are not user fields.
    if name.starts_with("iTun") {
        return;
    }
    for value in values {
        match name.to_ascii_uppercase().as_str() {
            "PERFORMER" => match split_performer(value) {
                (n, Some(role)) => b.push(&format!("performer:{role}"), n),
                (n, None) => b.push("performer", n),
            },
            "FMPS_RATING" | "RATING" => {
                if let Some(r) = normalise_rating(value) {
                    b.push("rating", &r);
                }
            }
            "FMPS_PLAYCOUNT" => b.push("playcount", value),
            _ => match (mean, fields::lookup(MP4_FREEFORM, name)) {
                ("com.apple.iTunes", Some(field)) => b.push(field, value),
                ("com.apple.iTunes", None) => b.push_custom(name, value),
                (other, _) => b.push_custom(&format!("{other}:{name}"), value),
            },
        }
    }
}
