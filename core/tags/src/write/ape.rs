//! Edits to an APEv2 tag (WavPack, Monkey's Audio, some MP3s).

use lofty::ape::{ApeItem, ApeTag};
use lofty::tag::ItemValue;

use super::Plan;
use super::vorbis::fmps_rating;
use crate::fields::{APE_EXTRA, VORBIS};
use crate::{Error, Result, split_performer};

pub(crate) fn apply(tag: &mut ApeTag, plan: &Plan) -> Result<()> {
    for field in plan.changes.keys() {
        remove(tag, field);
    }
    let first = |field: &str| {
        plan.changes
            .get(field)
            .and_then(|v| v.first())
            .map(String::as_str)
    };
    let mut performers: Vec<String> = Vec::new();
    for (field, values) in &plan.changes {
        if values.is_empty() {
            continue;
        }
        let field = field.as_str();
        let (key, value) = match field {
            "tracknumber" | "discnumber" => {
                let (key, total) = if field == "tracknumber" {
                    ("Track", "tracktotal")
                } else {
                    ("Disc", "disctotal")
                };
                match first(total) {
                    Some(t) => (key.to_owned(), format!("{}/{t}", values[0])),
                    None => (key.to_owned(), values[0].clone()),
                }
            }
            // Written with their number.
            "tracktotal" | "disctotal" => continue,
            "rating" => ("FMPS_RATING".to_owned(), fmps_rating(&values[0])),
            "playcount" => ("FMPS_PLAYCOUNT".to_owned(), values[0].clone()),
            "performer" => {
                performers.extend(values.iter().cloned());
                continue;
            }
            f if f.starts_with("performer:") => {
                let role = &f["performer:".len()..];
                performers.extend(values.iter().map(|v| format!("{v} ({role})")));
                continue;
            }
            f if f.starts_with("custom:") => (f["custom:".len()..].to_owned(), values.join("\0")),
            f => (key_for(f)?.to_owned(), values.join("\0")),
        };
        insert(tag, key, value)?;
    }
    if !performers.is_empty() {
        let mut all: Vec<String> = text_values(tag, "Performer");
        all.extend(performers);
        insert(tag, "Performer".to_owned(), all.join("\0"))?;
    }
    if let Some(pictures) = &plan.pictures {
        let covers: Vec<String> = (&*tag)
            .into_iter()
            .map(|i| i.key().to_owned())
            .filter(|k| k.starts_with("Cover Art ("))
            .collect();
        for key in covers {
            tag.remove(&key);
        }
        for p in pictures {
            let key = match p.picture_type {
                3 => "Cover Art (Front)",
                4 => "Cover Art (Back)",
                8 => "Cover Art (Artist)",
                6 => "Cover Art (Media)",
                _ => "Cover Art (Other)",
            };
            let ext = if p.data.starts_with(&[0x89, b'P', b'N', b'G']) {
                "png"
            } else {
                "jpg"
            };
            let name = if p.description.is_empty() {
                format!("cover.{ext}")
            } else {
                p.description.clone()
            };
            let mut data = name.into_bytes();
            data.push(0);
            data.extend_from_slice(&p.data);
            let item = ApeItem::new(key.to_owned(), ItemValue::Binary(data))
                .map_err(|e| Error::Unwritable(e.to_string()))?;
            tag.insert(item);
        }
    }
    Ok(())
}

fn insert(tag: &mut ApeTag, key: String, value: String) -> Result<()> {
    let item =
        ApeItem::new(key, ItemValue::Text(value)).map_err(|e| Error::Unwritable(e.to_string()))?;
    tag.insert(item);
    Ok(())
}

fn key_for(field: &str) -> Result<&'static str> {
    APE_EXTRA
        .iter()
        .chain(VORBIS)
        .find(|(_, mapped)| *mapped == field)
        .map(|(k, _)| *k)
        .ok_or_else(|| Error::Unwritable(format!("{field} has no APE item")))
}

fn text_values(tag: &ApeTag, key: &str) -> Vec<String> {
    tag.get(key)
        .and_then(|i| i.text_values())
        .map(|v| v.map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Removes every item the reader would turn into `field`.
fn remove(tag: &mut ApeTag, field: &str) {
    if field == "performer" || field.starts_with("performer:") {
        let role = field.strip_prefix("performer:");
        let kept: Vec<String> = text_values(tag, "Performer")
            .into_iter()
            .filter(|v| split_performer(v).1 != role)
            .collect();
        tag.remove("Performer");
        if !kept.is_empty() {
            let _ = insert(tag, "Performer".to_owned(), kept.join("\0"));
        }
        return;
    }
    let keys: Vec<String> = match field {
        "tracknumber" | "tracktotal" => vec![
            "Track".into(),
            "TRACKNUMBER".into(),
            "TRACKTOTAL".into(),
            "TOTALTRACKS".into(),
        ],
        "discnumber" | "disctotal" => vec![
            "Disc".into(),
            "DISCNUMBER".into(),
            "DISCTOTAL".into(),
            "TOTALDISCS".into(),
        ],
        "rating" => vec!["FMPS_RATING".into(), "RATING".into()],
        "playcount" => vec!["FMPS_PLAYCOUNT".into(), "PLAYCOUNT".into()],
        f if f.starts_with("custom:") => vec![f["custom:".len()..].to_owned()],
        f => APE_EXTRA
            .iter()
            .chain(VORBIS)
            .filter(|(_, mapped)| *mapped == f)
            .map(|(k, _)| (*k).to_owned())
            .collect(),
    };
    for key in keys {
        tag.remove(&key);
    }
}
