//! Edits to MP4 `ilst` atoms (AAC, ALAC).

use std::borrow::Cow;

use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst};
use lofty::tag::Accessor;

use super::vorbis::fmps_rating;
use super::{Plan, lofty_picture};
use crate::fields::{MP4_ATOMS, MP4_FREEFORM};
use crate::{Error, Result, split_performer};

const ITUNES: &str = "com.apple.iTunes";

pub(crate) fn apply(ilst: &mut Ilst, plan: &Plan) -> Result<()> {
    for field in plan.changes.keys() {
        remove(ilst, field);
    }
    let number = |field: &str| -> u32 {
        plan.changes
            .get(field)
            .and_then(|v| v.first())
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    };
    let mut performers: Vec<String> = Vec::new();
    for (field, values) in &plan.changes {
        if values.is_empty() {
            continue;
        }
        let field = field.as_str();
        match field {
            // Numbers: 0 means "none" in these atoms, so a 0 is left out.
            "tracknumber" | "tracktotal" | "discnumber" | "disctotal" => {
                let n = number(field);
                if n > 0 {
                    match field {
                        "tracknumber" => ilst.set_track(n),
                        "tracktotal" => ilst.set_track_total(n),
                        "discnumber" => ilst.set_disk(n),
                        _ => ilst.set_disk_total(n),
                    }
                }
            }
            "bpm" | "movement" | "movementtotal" => {
                let n = number(field);
                if n > 0 {
                    let code = match field {
                        "bpm" => *b"tmpo",
                        "movement" => *b"\xa9mvi",
                        _ => *b"\xa9mvc",
                    };
                    ilst.insert(Atom::new(
                        AtomIdent::Fourcc(code),
                        AtomData::SignedInteger(n as i32),
                    ));
                }
            }
            "compilation" => ilst.insert(Atom::new(
                AtomIdent::Fourcc(*b"cpil"),
                AtomData::Bool(values[0] == "1"),
            )),
            "rating" | "playcount" => {
                if number(field) > 0 {
                    let (name, value) = if field == "rating" {
                        ("FMPS_Rating", fmps_rating(&values[0]))
                    } else {
                        ("FMPS_Playcount", values[0].trim().to_owned())
                    };
                    insert(ilst, freeform(ITUNES, name), &[value]);
                }
            }
            "performer" => performers.extend(values.iter().cloned()),
            f if f.starts_with("performer:") => {
                let role = &f["performer:".len()..];
                performers.extend(values.iter().map(|v| format!("{v} ({role})")));
            }
            f if f.starts_with("custom:") => {
                insert(ilst, custom_ident(&f["custom:".len()..])?, values);
            }
            f => insert(ilst, ident_for(f)?, values),
        }
    }
    if !performers.is_empty() {
        let mut all = performer_values(ilst);
        all.extend(performers);
        ilst.retain(|a| !matches_freeform(a.ident(), "PERFORMER"));
        insert(ilst, freeform(ITUNES, "PERFORMER"), &all);
    }
    if let Some(pictures) = &plan.pictures {
        ilst.remove_pictures();
        for p in pictures {
            ilst.insert_picture(lofty_picture(p));
        }
    }
    Ok(())
}

fn insert(ilst: &mut Ilst, ident: AtomIdent<'static>, values: &[String]) {
    let data = values.iter().map(|v| AtomData::UTF8(v.clone())).collect();
    if let Some(atom) = Atom::from_collection(ident, data) {
        ilst.insert(atom);
    }
}

fn freeform(mean: &str, name: &str) -> AtomIdent<'static> {
    AtomIdent::Freeform {
        mean: Cow::Owned(mean.to_owned()),
        name: Cow::Owned(name.to_owned()),
    }
}

/// The atom a field is written to: a fixed four-character code, else an iTunes freeform atom.
fn ident_for(field: &str) -> Result<AtomIdent<'static>> {
    if let Some((code, _)) = MP4_ATOMS.iter().find(|(_, f)| *f == field) {
        return fourcc(code);
    }
    MP4_FREEFORM
        .iter()
        .find(|(_, f)| *f == field)
        .map(|(name, _)| freeform(ITUNES, name))
        .ok_or_else(|| Error::Unwritable(format!("{field} has no MP4 atom")))
}

/// "©xyz" is a four-character atom; "mean:name" a freeform atom of another vendor; anything
/// else an iTunes freeform atom. Mirrors how the reader names unknown atoms.
fn custom_ident(name: &str) -> Result<AtomIdent<'static>> {
    if name.chars().count() == 4 && name.chars().all(|c| (c as u32) < 256) {
        return fourcc(name);
    }
    Ok(match name.rsplit_once(':') {
        Some((mean, n)) if mean.contains('.') => freeform(mean, n),
        _ => freeform(ITUNES, name),
    })
}

fn fourcc(code: &str) -> Result<AtomIdent<'static>> {
    let bytes: Vec<u8> = code.chars().map(|c| c as u32 as u8).collect();
    let code: [u8; 4] = bytes
        .try_into()
        .map_err(|_| Error::Unwritable(format!("{code} isn't a four-character atom")))?;
    Ok(AtomIdent::Fourcc(code))
}

fn performer_values(ilst: &Ilst) -> Vec<String> {
    ilst.get(&freeform(ITUNES, "PERFORMER"))
        .map(|a| {
            a.data()
                .filter_map(|d| match d {
                    AtomData::UTF8(s) | AtomData::UTF16(s) => Some(s.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn matches_freeform(ident: &AtomIdent<'_>, want: &str) -> bool {
    matches!(ident, AtomIdent::Freeform { mean, name }
        if mean == ITUNES && name.eq_ignore_ascii_case(want))
}

/// Removes every atom the reader would turn into `field`.
fn remove(ilst: &mut Ilst, field: &str) {
    if field == "performer" || field.starts_with("performer:") {
        let role = field.strip_prefix("performer:");
        let kept: Vec<String> = performer_values(ilst)
            .into_iter()
            .filter(|v| split_performer(v).1 != role)
            .collect();
        ilst.retain(|a| !matches_freeform(a.ident(), "PERFORMER"));
        insert(ilst, freeform(ITUNES, "PERFORMER"), &kept);
        return;
    }
    let codes: Vec<String> = match field {
        "tracknumber" | "tracktotal" => vec!["trkn".into()],
        "discnumber" | "disctotal" => vec!["disk".into()],
        "bpm" => vec!["tmpo".into()],
        "movement" => vec!["©mvi".into()],
        "movementtotal" => vec!["©mvc".into()],
        "compilation" => vec!["cpil".into()],
        "genre" => vec!["©gen".into(), "gnre".into()],
        f if f.starts_with("custom:") && f.chars().count() == "custom:".len() + 4 => {
            vec![f["custom:".len()..].to_owned()]
        }
        f => MP4_ATOMS
            .iter()
            .filter(|(_, m)| *m == f)
            .map(|(c, _)| (*c).to_owned())
            .collect(),
    };
    let names: Vec<String> = match field {
        "rating" => vec!["FMPS_RATING".into(), "RATING".into()],
        "playcount" => vec!["FMPS_PLAYCOUNT".into()],
        f if f.starts_with("custom:") => vec![f["custom:".len()..].to_owned()],
        f => MP4_FREEFORM
            .iter()
            .filter(|(_, m)| *m == f)
            .map(|(n, _)| (*n).to_owned())
            .collect(),
    };
    let custom_other = field
        .strip_prefix("custom:")
        .and_then(|c| c.rsplit_once(':'));
    ilst.retain(|atom| match atom.ident() {
        AtomIdent::Fourcc(code) => {
            let code: String = code.iter().map(|&c| c as char).collect();
            !codes.contains(&code)
        }
        AtomIdent::Freeform { mean, name } => {
            let ours = mean == ITUNES && names.iter().any(|n| n.eq_ignore_ascii_case(name));
            let other = custom_other.is_some_and(|(m, n)| mean == m && name == n);
            !(ours || other)
        }
    });
}
