//! Edits to an ID3v2 tag (MP3, WAV, AIFF, DSF/DFF, and APE files tagged with ID3).

use std::borrow::Cow;

use lofty::TextEncoding;
use lofty::id3::v2::{
    CommentFrame, ExtendedTextFrame, Frame, FrameId, Id3v2Tag, KeyValueFrame, PopularimeterFrame,
    TextInformationFrame, TimestampFrame, UniqueFileIdentifierFrame, UnsynchronizedTextFrame,
};
use lofty::tag::items::Timestamp;

use super::{Plan, lofty_picture};
use crate::fields::{self, ID3_TEXT, ID3_TXXX, INVOLVED_ROLES};
use crate::{Error, Result};

const MUSICBRAINZ_UFID: &str = "http://musicbrainz.org";

/// ID3v2.4 frames that ID3v2.3 lacks (lofty drops them from v2.3 tags), and the TXXX frame
/// written in their place, as other taggers do.
const V24_ONLY: &[(&str, &str)] = &[
    ("TSST", "DISCSUBTITLE"),
    ("TMOO", "MOOD"),
    ("TSOA", "ALBUMSORT"),
    ("TSOP", "ARTISTSORT"),
    ("TSOT", "TITLESORT"),
];
/// The POPM e-mail other taggers (Picard, mutagen) use when none is set.
const DEFAULT_POPM_EMAIL: &str = "no@email";

pub(crate) fn apply(tag: &mut Id3v2Tag, plan: &Plan, v23: bool) -> Result<()> {
    let popm_email = tag
        .iter()
        .find_map(|f| match f {
            Frame::Popularimeter(p) => Some(p.email.to_string()),
            _ => None,
        })
        .unwrap_or_else(|| DEFAULT_POPM_EMAIL.to_owned());

    for field in plan.changes.keys() {
        remove(tag, field);
    }
    let encoding = if v23 {
        TextEncoding::UTF16
    } else {
        TextEncoding::UTF8
    };
    let join = |values: &[String]| {
        if v23 {
            values.join("/")
        } else {
            values.join("\0")
        }
    };
    let first = |field: &str| {
        plan.changes
            .get(field)
            .and_then(|v| v.first())
            .map(String::as_str)
    };

    for (field, values) in &plan.changes {
        if values.is_empty() {
            continue;
        }
        let field = field.as_str();
        match field {
            "tracknumber" | "discnumber" | "movement" => {
                let (id, total) = match field {
                    "tracknumber" => ("TRCK", "tracktotal"),
                    "discnumber" => ("TPOS", "disctotal"),
                    _ => ("MVIN", "movementtotal"),
                };
                let value = match first(total) {
                    Some(t) => format!("{}/{t}", values[0]),
                    None => values[0].clone(),
                };
                tag.insert(text(id, encoding, value));
            }
            // Written with their number.
            "tracktotal" | "disctotal" | "movementtotal" => {}
            "rating" | "playcount" => {
                // One POPM frame holds both; write it once, from "rating".
                if field == "playcount" && plan.changes.contains_key("rating") {
                    continue;
                }
                insert_popm(tag, &popm_email, first("rating"), first("playcount"));
            }
            "comment" => {
                tag.insert(Frame::Comment(CommentFrame::new(
                    encoding,
                    *b"eng",
                    "",
                    values[0].clone(),
                )));
            }
            "lyrics" => {
                tag.insert(Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(
                    encoding,
                    *b"eng",
                    "",
                    values[0].clone(),
                )));
            }
            "musicbrainz_trackid" => {
                tag.insert(Frame::UniqueFileIdentifier(UniqueFileIdentifierFrame::new(
                    MUSICBRAINZ_UFID,
                    values[0].clone().into_bytes(),
                )));
            }
            "compilation" => {
                tag.insert(text("TCMP", encoding, values[0].clone()));
            }
            // Timestamp frames, which lofty turns into TYER/TDAT/TIME and TORY for ID3v2.3.
            "date" | "originaldate" => {
                let id = if field == "date" { "TDRC" } else { "TDOR" };
                let timestamp: Timestamp = values[0]
                    .parse()
                    .map_err(|_| Error::Unwritable(format!("{field} isn't a date")))?;
                tag.insert(Frame::Timestamp(TimestampFrame::new(
                    FrameId::Valid(Cow::Borrowed(id)),
                    encoding,
                    timestamp,
                )));
            }
            f if f.starts_with("performer:") => {
                let role = &f["performer:".len()..];
                add_pairs(tag, "TMCL", encoding, role, values);
            }
            f if f.starts_with("custom:") => {
                let name = &f["custom:".len()..];
                if let Some(desc) = name.strip_prefix("COMM:") {
                    tag.insert(Frame::Comment(CommentFrame::new(
                        encoding,
                        *b"eng",
                        desc.to_owned(),
                        values.join("\n"),
                    )));
                } else if is_text_frame_id(name) {
                    tag.insert(text(name, encoding, join(values)));
                } else {
                    tag.insert(Frame::UserText(ExtendedTextFrame::new(
                        encoding,
                        name.to_owned(),
                        join(values),
                    )));
                }
            }
            f => {
                if let Some(role) = involved_role(f) {
                    add_pairs(tag, "TIPL", encoding, role, values);
                } else if let Some(id) = text_id(f) {
                    match V24_ONLY.iter().find(|(v4, _)| *v4 == id) {
                        Some((_, desc)) if v23 => {
                            tag.insert(Frame::UserText(ExtendedTextFrame::new(
                                encoding,
                                (*desc).to_owned(),
                                join(values),
                            )));
                        }
                        _ => {
                            tag.insert(text(id, encoding, join(values)));
                        }
                    }
                } else if let Some(desc) = txxx_desc(f) {
                    tag.insert(Frame::UserText(ExtendedTextFrame::new(
                        encoding,
                        desc.to_owned(),
                        join(values),
                    )));
                } else {
                    return Err(Error::Unwritable(format!("{f} has no ID3 frame")));
                }
            }
        }
    }

    if let Some(pictures) = &plan.pictures {
        tag.retain(|f| !matches!(f, Frame::Picture(_)));
        for p in pictures {
            tag.insert_picture(lofty_picture(p));
        }
    }
    Ok(())
}

/// Removes everything the reader would turn into `field`.
fn remove(tag: &mut Id3v2Tag, field: &str) {
    let ids: Vec<&str> = match field {
        "tracknumber" | "tracktotal" => vec!["TRCK"],
        "discnumber" | "disctotal" => vec!["TPOS"],
        "movement" | "movementtotal" => vec!["MVIN"],
        f => ID3_TEXT
            .iter()
            .filter(|(_, mapped)| *mapped == f)
            .map(|(id, _)| *id)
            .collect(),
    };
    let custom = field.strip_prefix("custom:");
    let role = field.strip_prefix("performer:");
    let involved = involved_roles(field);
    tag.retain_mut(|frame| {
        let id = frame.id_str().to_owned();
        if ids.contains(&id.as_str()) || custom == Some(id.as_str()) {
            return false;
        }
        match frame {
            Frame::UserText(f) => {
                let desc = f.description.as_ref();
                !(fields::lookup(ID3_TXXX, desc) == Some(field)
                    || custom.is_some_and(|c| c.eq_ignore_ascii_case(desc)))
            }
            Frame::Comment(f) => {
                let desc = f.description.as_ref();
                !((field == "comment" && desc.is_empty())
                    || custom.and_then(|c| c.strip_prefix("COMM:")) == Some(desc))
            }
            Frame::UnsynchronizedText(_) => field != "lyrics",
            Frame::Popularimeter(_) => !matches!(field, "rating" | "playcount"),
            Frame::UniqueFileIdentifier(f) => {
                !(field == "musicbrainz_trackid" && f.owner == MUSICBRAINZ_UFID)
            }
            Frame::KeyValue(f) => {
                let musicians = id == "TMCL";
                let pairs: Vec<_> = f
                    .key_value_pairs
                    .iter()
                    .filter(|(r, _)| {
                        let r = r.as_ref();
                        let as_field = fields::lookup(INVOLVED_ROLES, r);
                        let is_performer = musicians || as_field.is_none();
                        let this_role = is_performer && role == Some(r);
                        let this_field = !musicians && involved.contains(&r);
                        !(this_role || this_field)
                    })
                    .cloned()
                    .collect();
                let keep = !pairs.is_empty();
                f.key_value_pairs = Cow::Owned(pairs);
                keep
            }
            _ => true,
        }
    });
}

fn text(id: &str, encoding: TextEncoding, value: String) -> Frame<'static> {
    Frame::Text(TextInformationFrame::new(
        FrameId::Valid(Cow::Owned(id.to_owned())),
        encoding,
        value,
    ))
}

fn insert_popm(tag: &mut Id3v2Tag, email: &str, rating: Option<&str>, count: Option<&str>) {
    let rating: u32 = rating.and_then(|r| r.trim().parse().ok()).unwrap_or(0);
    let count: u64 = count.and_then(|c| c.trim().parse().ok()).unwrap_or(0);
    if rating == 0 && count == 0 {
        return;
    }
    let byte = ((rating.min(100) as f64) * 255.0 / 100.0).round() as u8;
    tag.insert(Frame::Popularimeter(PopularimeterFrame::new(
        email.to_owned(),
        byte,
        count,
    )));
}

/// Adds (role, name) pairs to the TMCL (musicians) or TIPL (involved people) frame.
fn add_pairs(tag: &mut Id3v2Tag, id: &str, encoding: TextEncoding, role: &str, names: &[String]) {
    let mut pairs: Vec<(Cow<'static, str>, Cow<'static, str>)> = Vec::new();
    tag.retain(|f| match f {
        Frame::KeyValue(kv) if f.id_str() == id => {
            pairs.extend(
                kv.key_value_pairs
                    .iter()
                    .map(|(r, n)| (Cow::Owned(r.to_string()), Cow::Owned(n.to_string()))),
            );
            false
        }
        _ => true,
    });
    for name in names {
        pairs.push((Cow::Owned(role.to_owned()), Cow::Owned(name.clone())));
    }
    tag.insert(Frame::KeyValue(KeyValueFrame::new(
        FrameId::Valid(Cow::Owned(id.to_owned())),
        encoding,
        pairs,
    )));
}

/// The text frame a field is written to: the first listed for it.
fn text_id(field: &str) -> Option<&'static str> {
    ID3_TEXT
        .iter()
        .find(|(_, f)| *f == field)
        .map(|(id, _)| *id)
}

fn txxx_desc(field: &str) -> Option<&'static str> {
    ID3_TXXX.iter().find(|(_, f)| *f == field).map(|(d, _)| *d)
}

/// The TIPL role a field is written as, for fields without a text frame of their own.
fn involved_role(field: &str) -> Option<&'static str> {
    if text_id(field).is_some() {
        return None;
    }
    INVOLVED_ROLES
        .iter()
        .find(|(_, f)| *f == field)
        .map(|(role, _)| *role)
}

/// Every TIPL role the reader turns into `field`.
fn involved_roles(field: &str) -> Vec<&'static str> {
    INVOLVED_ROLES
        .iter()
        .filter(|(_, f)| *f == field)
        .map(|(role, _)| *role)
        .collect()
}

/// A four-character T*** frame id other than TXXX ("TDRL", "TSSE"...).
fn is_text_frame_id(name: &str) -> bool {
    name.len() == 4
        && name.starts_with('T')
        && name != "TXXX"
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}
