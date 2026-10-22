//! The write path: every tag format round-trips, unknown data survives, bad edits and crashes
//! never damage the original.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use cadenza_tags::{Edit, Error, Fields, Picture, TagFormat, WriteOptions, read, write};
use cadenza_testkit::group;

fn copy_fixture(id: &str, dir: &Path) -> PathBuf {
    let e = group("tags").find(|e| e.id == id).unwrap();
    let dst = dir.join(e.file().file_name().unwrap());
    std::fs::copy(e.file(), &dst).unwrap();
    dst
}

fn tag_fixtures() -> Vec<&'static str> {
    group("tags").map(|e| e.id.as_str()).collect()
}

fn set(pairs: &[(&str, &[&str])]) -> Edit {
    Edit {
        fields: pairs
            .iter()
            .map(|(f, v)| ((*f).to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
            .collect(),
        pictures: None,
    }
}

fn audio_hash(path: &Path) -> String {
    let t = read(path).unwrap();
    cadenza_tags::audio::content_hash(path, t.audio.container).unwrap()
}

fn without_derived(mut f: Fields) -> Fields {
    f.remove("musicbrainz_recordingid");
    f
}

#[test]
fn every_tag_format_round_trips() {
    let tmp = tempfile::tempdir().unwrap();
    let mut checked = 0;
    for id in tag_fixtures() {
        let path = copy_fixture(id, tmp.path());
        let before = read(&path).unwrap();
        let hash = audio_hash(&path);
        let v23 = before.format == TagFormat::Id3v23;

        let edit = set(&[
            ("title", &["Edited — 編集 ✓"]),
            ("artist", &["New One", "New Two"]),
            ("mood", &[]),
            ("custom:CADENZA_NOTE", &["kept"]),
            ("tracknumber", &["7"]),
        ]);
        let after =
            write(&path, &edit, &WriteOptions::default()).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(
            after.fields,
            read(&path).unwrap().fields,
            "{id}: returned tags"
        );

        let mut want = before.fields.clone();
        want.insert("title".into(), vec!["Edited — 編集 ✓".into()]);
        want.insert(
            "artist".into(),
            if v23 {
                vec!["New One/New Two".into()]
            } else {
                vec!["New One".into(), "New Two".into()]
            },
        );
        want.remove("mood");
        want.insert("custom:CADENZA_NOTE".into(), vec!["kept".into()]);
        want.insert("tracknumber".into(), vec!["7".into()]);
        assert_eq!(
            without_derived(after.fields.clone()),
            without_derived(want),
            "{id}: fields after the edit"
        );
        assert_eq!(after.pictures, before.pictures, "{id}: pictures");
        assert_eq!(audio_hash(&path), hash, "{id}: audio");

        // Putting the old values back restores the original tags.
        let mut undo = Edit::default();
        for field in [
            "title",
            "artist",
            "mood",
            "custom:CADENZA_NOTE",
            "tracknumber",
        ] {
            undo.fields.insert(
                field.into(),
                before.fields.get(field).cloned().unwrap_or_default(),
            );
        }
        let restored = write(&path, &undo, &WriteOptions::default())
            .unwrap_or_else(|e| panic!("{id} undo: {e}"));
        assert_eq!(
            without_derived(restored.fields),
            without_derived(before.fields),
            "{id}: undo"
        );
        checked += 1;
    }
    assert_eq!(checked, 13, "every tag format");
}

/// Frames, comments and atoms Cadenza has no field for come through a write unchanged.
#[test]
fn unknown_data_survives() {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    let tmp = tempfile::tempdir().unwrap();
    let opts = ParseOptions::new();

    let mp3 = copy_fixture("tags/mp3_id3v24", tmp.path());
    let frames = |p: &Path| {
        let f =
            lofty::mpeg::MpegFile::read_from(&mut std::fs::File::open(p).unwrap(), opts).unwrap();
        let mut ids: Vec<String> = f
            .id3v2()
            .unwrap()
            .iter()
            .map(|f| f.id_str().to_owned())
            .collect();
        ids.sort();
        ids
    };
    let before = frames(&mp3);
    write(&mp3, &set(&[("title", &["New"])]), &WriteOptions::default()).unwrap();
    assert_eq!(frames(&mp3), before, "ID3 frames");

    let flac = copy_fixture("tags/flac_vorbis", tmp.path());
    let keys = |p: &Path| {
        let f =
            lofty::flac::FlacFile::read_from(&mut std::fs::File::open(p).unwrap(), opts).unwrap();
        let mut k: Vec<String> = f
            .vorbis_comments()
            .unwrap()
            .items()
            .map(|(k, _)| k.to_owned())
            .collect();
        k.retain(|k| k != "TITLE");
        k.sort();
        k
    };
    let before = keys(&flac);
    write(
        &flac,
        &set(&[("title", &["New"])]),
        &WriteOptions::default(),
    )
    .unwrap();
    assert_eq!(keys(&flac), before, "Vorbis comments");

    let m4a = copy_fixture("tags/m4a_aac", tmp.path());
    let atoms = |p: &Path| {
        let f = lofty::mp4::Mp4File::read_from(&mut std::fs::File::open(p).unwrap(), opts).unwrap();
        let mut a: Vec<String> = f
            .ilst()
            .unwrap()
            .into_iter()
            .map(|a| format!("{:?}", a.ident()))
            .collect();
        a.sort();
        a
    };
    let before = atoms(&m4a);
    write(&m4a, &set(&[("title", &["New"])]), &WriteOptions::default()).unwrap();
    assert_eq!(atoms(&m4a), before, "MP4 atoms");
}

#[test]
fn pictures_are_replaced() {
    let tmp = tempfile::tempdir().unwrap();
    let png = {
        let e = cadenza_testkit::manifest()
            .entries
            .iter()
            .find(|e| !e.artwork.is_empty())
            .unwrap();
        read(&e.file())
            .unwrap()
            .pictures
            .into_iter()
            .next()
            .unwrap()
    };
    let new = vec![
        Picture {
            picture_type: 3,
            ..png.clone()
        },
        Picture {
            picture_type: 4,
            description: "back".into(),
            ..png.clone()
        },
    ];
    for id in tag_fixtures() {
        let path = copy_fixture(id, tmp.path());
        let edit = Edit {
            pictures: Some(new.clone()),
            ..Edit::default()
        };
        let after =
            write(&path, &edit, &WriteOptions::default()).unwrap_or_else(|e| panic!("{id}: {e}"));
        let data: Vec<&[u8]> = after.pictures.iter().map(|p| p.data.as_slice()).collect();
        assert_eq!(data, vec![png.data.as_slice(); 2], "{id}");
        let none = Edit {
            pictures: Some(Vec::new()),
            ..Edit::default()
        };
        assert!(
            write(&path, &none, &WriteOptions::default())
                .unwrap()
                .pictures
                .is_empty(),
            "{id}: removed"
        );
    }
}

#[test]
fn refused_edits_leave_the_file_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let path = copy_fixture("tags/flac_vorbis", tmp.path());
    let bytes = std::fs::read(&path).unwrap();
    let bad = [
        set(&[("not-a-field", &["x"])]),
        set(&[("tracknumber", &["seven"])]),
        set(&[("rating", &["101"])]),
        set(&[("comment", &["one", "two"])]),
        set(&[("syncedlyrics", &["[00:01.00]hi"])]),
    ];
    for edit in bad {
        let err = write(&path, &edit, &WriteOptions::default()).unwrap_err();
        assert!(matches!(err, Error::Unwritable(_)), "{edit:?}: {err}");
    }
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms).unwrap();
    let err = write(&path, &set(&[("title", &["x"])]), &WriteOptions::default()).unwrap_err();
    assert!(matches!(err, Error::ReadOnly));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let left: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
    assert_eq!(left.len(), 1, "no temporary files left");
}

#[test]
fn backups_keep_the_original() {
    let tmp = tempfile::tempdir().unwrap();
    let path = copy_fixture("tags/opus", tmp.path());
    let original = std::fs::read(&path).unwrap();
    let backup = tmp.path().join("backups/opus.opus");
    let options = WriteOptions {
        backup: Some(backup.clone()),
    };
    write(&path, &set(&[("album", &["Changed"])]), &options).unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    assert_ne!(std::fs::read(&path).unwrap(), original);
}

/// Child half of `a_crash_mid_write_never_damages_the_file`: writes, and the write aborts the
/// process at the stage named in CADENZA_TAGS_CRASH_AT.
#[test]
fn crash_child() {
    let Ok(path) = std::env::var("CADENZA_TAGS_CRASH_FILE") else {
        return;
    };
    let _ = write(
        Path::new(&path),
        &set(&[("title", &["After"])]),
        &WriteOptions::default(),
    );
}

#[test]
fn a_crash_mid_write_never_damages_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    for id in [
        "tags/flac_vorbis",
        "tags/mp3_id3v24",
        "tags/dsf_id3",
        "tags/m4a_alac",
    ] {
        for stage in ["copied", "edited", "verified", "replaced"] {
            let dir = tmp.path().join(format!("{}-{stage}", id.replace('/', "_")));
            std::fs::create_dir_all(&dir).unwrap();
            let path = copy_fixture(id, &dir);
            let before = read(&path).unwrap();
            let hash = audio_hash(&path);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "crash_child", "--nocapture", "--test-threads=1"])
                .env("CADENZA_TAGS_CRASH_AT", stage)
                .env("CADENZA_TAGS_CRASH_FILE", &path)
                .status()
                .unwrap();
            assert!(
                !status.success(),
                "{id} {stage}: the child should have aborted"
            );

            let now = read(&path).unwrap_or_else(|e| panic!("{id} {stage}: unreadable: {e}"));
            let title = now.get("title").unwrap();
            if stage == "replaced" {
                assert_eq!(title, "After", "{id} {stage}");
            } else {
                assert_eq!(
                    now.fields, before.fields,
                    "{id} {stage}: original untouched"
                );
            }
            assert_eq!(audio_hash(&path), hash, "{id} {stage}: audio");
            let stale = cadenza_tags::remove_stale_temps(&dir).unwrap();
            let expected = usize::from(stage != "replaced");
            assert_eq!(stale, expected, "{id} {stage}: leftover temporary files");
        }
    }
}

/// Every field Cadenza knows, set at once in every format; the write's own verification checks
/// each reads back as intended.
#[test]
fn every_field_can_be_rewritten() {
    let tmp = tempfile::tempdir().unwrap();
    for id in tag_fixtures() {
        let path = copy_fixture(id, tmp.path());
        let id3 = matches!(
            read(&path).unwrap().format,
            TagFormat::Id3v23 | TagFormat::Id3v24
        );
        let mut edit = Edit::default();
        for &field in cadenza_tags::fields::FIELDS {
            let value = match field {
                "syncedlyrics" => continue,
                "musicbrainz_recordingid" => continue,
                "date" | "originaldate" => "2020-01-02",
                "performer" if id3 => continue,
                "tracknumber" | "discnumber" | "movement" => "2",
                "tracktotal" | "disctotal" | "movementtotal" => "9",
                "compilation" => "0",
                "rating" => "40",
                "playcount" | "bpm" => "33",
                f => f,
            };
            edit.fields.insert(field.to_owned(), vec![value.to_owned()]);
        }
        edit.fields.insert(
            "performer:piano".into(),
            vec!["Pia No".into(), "Second Pianist".into()],
        );
        edit.fields.insert("performer:violin".into(), Vec::new());
        edit.fields
            .insert("genre".into(), vec!["Jazz".into(), "Blues".into()]);
        let after =
            write(&path, &edit, &WriteOptions::default()).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(after.get("albumartist"), Some("albumartist"), "{id}");
        assert_eq!(after.get("rating"), Some("40"), "{id}");
        assert!(!after.fields.contains_key("performer:violin"), "{id}");
    }
}

/// Another tagger's date frame that isn't an ID3 timestamp ("Spring 1999") must not make the
/// whole file unreadable, nor uneditable.
#[test]
fn a_file_with_a_bad_date_frame_still_reads_and_edits() {
    use lofty::config::{ParseOptions, WriteOptions as LoftyWrite};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, FrameId, TextInformationFrame};
    let tmp = tempfile::tempdir().unwrap();
    let path = copy_fixture("tags/mp3_id3v24", tmp.path());
    let mut f = lofty::mpeg::MpegFile::read_from(
        &mut std::fs::File::open(&path).unwrap(),
        ParseOptions::new(),
    )
    .unwrap();
    f.id3v2_mut()
        .unwrap()
        .insert(Frame::Text(TextInformationFrame::new(
            FrameId::Valid(std::borrow::Cow::Borrowed("TDRC")),
            lofty::TextEncoding::UTF8,
            "Spring 1999",
        )));
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    f.save_to(&mut file, LoftyWrite::new()).unwrap();

    let t = read(&path).unwrap();
    assert_eq!(t.get("album"), Some("Fixture Album"));
    let after = write(
        &path,
        &set(&[("album", &["Edited"])]),
        &WriteOptions::default(),
    )
    .unwrap();
    assert_eq!(after.get("album"), Some("Edited"));
    let err = write(
        &path,
        &set(&[("date", &["Spring 1999"])]),
        &WriteOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Unwritable(_)), "{err}");
}
