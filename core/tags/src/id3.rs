//! ID3v2 (2.2/2.3/2.4), ID3v1 and RIFF INFO → canonical fields.

use lofty::TextEncoding;
use lofty::id3::v1::Id3v1Tag;
use lofty::id3::v2::{Frame, Id3v2Tag, Id3v2Version, SynchronizedTextFrame};
use lofty::iff::wav::RiffInfoList;

use crate::fields::{self, ID3_TEXT, ID3_TXXX, INVOLVED_ROLES};
use crate::legacy::{self, LegacyText};
use crate::{Fields, FieldsBuilder, Picture, TagFormat};

pub(crate) fn read(
    tag: &Id3v2Tag,
    legacy: &mut Vec<LegacyText>,
) -> (TagFormat, Fields, Vec<Picture>) {
    let format = match tag.original_version() {
        Id3v2Version::V2 => TagFormat::Id3v22,
        Id3v2Version::V3 => TagFormat::Id3v23,
        Id3v2Version::V4 => TagFormat::Id3v24,
    };
    let mut b = FieldsBuilder::default();
    let mut pictures = Vec::new();
    let mut have_rating = false;

    for frame in tag {
        let id = frame.id_str();
        match frame {
            Frame::Text(f) => {
                let value = f.value.as_ref();
                note_legacy(legacy, f.encoding, id, value);
                match id {
                    "TRCK" => b.push_number_pair("tracknumber", "tracktotal", value),
                    "TPOS" => b.push_number_pair("discnumber", "disctotal", value),
                    "MVIN" => b.push_number_pair("movement", "movementtotal", value),
                    "TCON" => {
                        for g in split_values(value) {
                            b.push("genre", &resolve_genre(g));
                        }
                    }
                    "TCMP" => b.push("compilation", if value.trim() == "0" { "0" } else { "1" }),
                    _ => match fields::lookup(ID3_TEXT, id) {
                        Some(field) => split_values(value).for_each(|v| b.push(field, v)),
                        None => split_values(value).for_each(|v| b.push_custom(id, v)),
                    },
                }
            }
            Frame::Timestamp(f) => {
                let field = match id {
                    "TDRC" => "date",
                    "TDOR" => "originaldate",
                    "TDRL" => "custom:TDRL",
                    _ => continue,
                };
                b.push(field, &f.timestamp.to_string());
            }
            Frame::UserText(f) => {
                let desc = f.description.as_ref();
                note_legacy(legacy, f.encoding, &format!("TXXX:{desc}"), &f.content);
                match fields::lookup(ID3_TXXX, desc) {
                    Some(field) => split_values(&f.content).for_each(|v| b.push(field, v)),
                    None => split_values(&f.content).for_each(|v| b.push_custom(desc, v)),
                }
            }
            Frame::Comment(f) => {
                note_legacy(legacy, f.encoding, "COMM", &f.content);
                let desc = f.description.as_ref();
                if desc.is_empty() {
                    b.push("comment", &f.content);
                } else if !desc.starts_with("iTun") {
                    b.push_custom(&format!("COMM:{desc}"), &f.content);
                }
            }
            Frame::UnsynchronizedText(f) => {
                if !b.fields.contains_key("lyrics") {
                    b.push("lyrics", &f.content);
                }
            }
            Frame::Popularimeter(f) => {
                // The first POPM wins; ratings from other players' emails are ignored.
                if !have_rating {
                    have_rating = true;
                    if f.rating > 0 {
                        b.push(
                            "rating",
                            &((f.rating as f64) * 100.0 / 255.0).round().to_string(),
                        );
                    }
                    if f.counter > 0 {
                        b.push("playcount", &f.counter.to_string());
                    }
                }
            }
            Frame::UniqueFileIdentifier(f) => {
                if f.owner == "http://musicbrainz.org" {
                    b.push(
                        "musicbrainz_recordingid",
                        &String::from_utf8_lossy(&f.identifier),
                    );
                    if !b.fields.contains_key("musicbrainz_trackid") {
                        b.push(
                            "musicbrainz_trackid",
                            &String::from_utf8_lossy(&f.identifier),
                        );
                    }
                }
            }
            Frame::KeyValue(f) => {
                // TMCL: musician credits (instrument → name). TIPL/IPLS: involved people.
                for (role, name) in f.key_value_pairs.iter() {
                    match (id, fields::lookup(INVOLVED_ROLES, role)) {
                        ("TMCL", _) | (_, None) => b.push(&format!("performer:{role}"), name),
                        (_, Some(field)) => b.push(field, name),
                    }
                }
            }
            Frame::Picture(f) => pictures.push(Picture {
                picture_type: f.picture.pic_type().as_u8(),
                mime: f
                    .picture
                    .mime_type()
                    .map(|m| m.as_str().to_owned())
                    .unwrap_or_default(),
                description: f.picture.description().unwrap_or_default().to_owned(),
                data: f.picture.data().to_vec(),
            }),
            Frame::Binary(f) if id == "SYLT" => {
                if let Ok(sylt) = SynchronizedTextFrame::parse(&f.data, frame.flags())
                    && let Some(lrc) = sylt_to_lrc(&sylt)
                {
                    b.push("syncedlyrics", &lrc);
                }
            }
            _ => {}
        }
    }
    let fields = b.finish();
    (format, fields, pictures)
}

/// ID3v2.4 separates multiple values with NUL. ID3v2.3 has no multi-value convention; values
/// joined with "/" stay one value (splitting would break "AC/DC").
fn split_values(value: &str) -> impl Iterator<Item = &str> {
    value.split('\0').filter(|v| !v.is_empty())
}

/// "(13)", "13", "(13)Pop" and "Rock" → genre names; "(RX)" and "(CR)" are ID3's Remix/Cover.
fn resolve_genre(value: &str) -> String {
    let v = value.trim();
    let index = |s: &str| {
        s.parse::<usize>()
            .ok()
            .and_then(|i| lofty::id3::v1::GENRES.get(i))
    };
    if let Some(name) = index(v) {
        return (*name).to_owned();
    }
    if let Some(rest) = v.strip_prefix('(')
        && let Some((inner, tail)) = rest.split_once(')')
    {
        if !tail.is_empty() {
            return tail.to_owned();
        }
        match inner {
            "RX" => return "Remix".to_owned(),
            "CR" => return "Cover".to_owned(),
            _ => {
                if let Some(name) = index(inner) {
                    return (*name).to_owned();
                }
            }
        }
    }
    v.to_owned()
}

fn sylt_to_lrc(sylt: &SynchronizedTextFrame<'_>) -> Option<String> {
    use lofty::id3::v2::TimestampFormat;
    if sylt.timestamp_format != TimestampFormat::MS {
        return None;
    }
    let mut out = String::new();
    for (ms, text) in &sylt.content {
        let (m, s, cs) = (ms / 60_000, (ms / 1000) % 60, (ms % 1000) / 10);
        out.push_str(&format!("[{m:02}:{s:02}.{cs:02}]{text}\n"));
    }
    (!out.is_empty()).then_some(out)
}

fn note_legacy(legacy: &mut Vec<LegacyText>, encoding: TextEncoding, field: &str, value: &str) {
    if encoding == TextEncoding::Latin1
        && let Some(found) = legacy::detect(field, value)
    {
        legacy.push(found);
    }
}

pub(crate) fn read_v1(tag: &Id3v1Tag, legacy: &mut Vec<LegacyText>) -> Fields {
    let mut b = FieldsBuilder::default();
    let mut put = |field: &str, value: Option<&str>| {
        if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
            if let Some(found) = legacy::detect(field, v) {
                legacy.push(found);
            }
            b.push(field, v);
        }
    };
    put("title", tag.title.as_deref());
    put("artist", tag.artist.as_deref());
    put("album", tag.album.as_deref());
    let year = tag.year.map(|y| y.to_string());
    put("date", year.as_deref());
    put("comment", tag.comment.as_deref());
    if let Some(n) = tag.track_number {
        b.push("tracknumber", &n.to_string());
    }
    if let Some(name) = tag
        .genre
        .and_then(|g| lofty::id3::v1::GENRES.get(g as usize))
    {
        b.push("genre", name);
    }
    b.finish()
}

pub(crate) fn riff_info(info: &RiffInfoList) -> Fields {
    let mut b = FieldsBuilder::default();
    for (key, value) in info {
        let field = match key.as_str() {
            "INAM" => "title",
            "IART" => "artist",
            "IPRD" => "album",
            "ICMT" => "comment",
            "ICRD" => "date",
            "IGNR" => "genre",
            "ITRK" | "IPRT" => "tracknumber",
            "ICOP" => "copyright",
            "ISFT" => "encodersettings",
            other => {
                b.push_custom(other, value);
                continue;
            }
        };
        b.push(field, value);
    }
    b.finish()
}
