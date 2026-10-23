//! The remote stores against real servers: WebDAV and S3. They run when the servers are given:
//!
//! - `CADENZA_TEST_WEBDAV=<url>|<user>|<password>` (e.g. wsgidav)
//! - `CADENZA_TEST_S3=<endpoint>|<bucket>|<access key>|<secret>` (e.g. moto or MinIO)
//!
//! `cargo xtask ci` starts local servers and sets these; otherwise the tests say they're skipped.

use std::path::Path;

use cadenza_sync::crypt::{EncryptedStore, Key};
use cadenza_sync::{Change, DeviceId, Remote, RemoteStore, Replica, Store, TrackKey};

/// A fresh, unique folder on the server for each test run.
fn unique() -> String {
    format!("t{}", uuid::Uuid::now_v7().simple())
}

fn webdav() -> Option<RemoteStore> {
    let spec = std::env::var("CADENZA_TEST_WEBDAV").ok()?;
    let mut parts = spec.split('|');
    let (url, user, pass) = (parts.next()?, parts.next()?, parts.next()?);
    let remote = Remote::WebDav {
        url: format!("{}/{}", url.trim_end_matches('/'), unique()),
        username: user.into(),
    };
    Some(RemoteStore::open(&remote, pass, None).expect("webdav"))
}

fn s3() -> Option<RemoteStore> {
    let spec = std::env::var("CADENZA_TEST_S3").ok()?;
    let mut parts = spec.split('|');
    let (endpoint, bucket, key, secret) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let remote = Remote::S3 {
        endpoint: endpoint.into(),
        bucket: bucket.into(),
        region: "us-east-1".into(),
        access_key_id: key.into(),
        root: unique(),
    };
    Some(RemoteStore::open(&remote, secret, None).expect("s3"))
}

fn contract(store: &dyn Store, scratch: &Path) {
    assert_eq!(store.list("log").expect("list"), Vec::<String>::new());
    assert_eq!(store.get("log/a/0.seg").expect("get"), None);
    assert_eq!(store.size("log/a/0.seg").expect("size"), None);
    store.put("log/b/0000.seg", b"two").expect("put");
    store.put("log/a/0001.seg", b"one").expect("put");
    store.put("log/a/0000.seg", b"zero").expect("put");
    store
        .put("log/a/0000.seg", b"zero again")
        .expect("overwrite");
    assert_eq!(store.list("log").expect("list"), ["a", "b"]);
    assert_eq!(store.list("log/a").expect("list"), ["0000.seg", "0001.seg"]);
    assert_eq!(
        store.get("log/a/0000.seg").expect("get").as_deref(),
        Some(&b"zero again"[..])
    );
    assert_eq!(store.size("log/a/0001.seg").expect("size"), Some(3));

    // A song: 20 MiB and a bit, so it takes several chunks.
    let song: Vec<u8> = (0..(20 << 20) + 77).map(|i| (i % 251) as u8).collect();
    let src = scratch.join("song.flac");
    std::fs::write(&src, &song).expect("write");
    // Stopped before it starts (a WebDAV upload is one request, so that's where it can stop),
    // nothing appears.
    let stopped = store
        .put_file("blobs/ab/abc.flac", &src, &mut |_| false)
        .expect("put");
    assert!(!stopped);
    assert_eq!(
        store.size("blobs/ab/abc.flac").expect("size"),
        None,
        "an unfinished upload isn't there"
    );
    assert!(
        store
            .put_file("blobs/ab/abc.flac", &src, &mut |_| true)
            .expect("put")
    );
    assert_eq!(
        store.size("blobs/ab/abc.flac").expect("size"),
        Some(song.len() as u64)
    );

    // A download stopped part-way resumes where it stopped.
    let dest = scratch.join("copy.flac");
    let mut reached = 0;
    let got = store
        .get_file("blobs/ab/abc.flac", &dest, &mut |n| {
            reached = n;
            n < (8 << 20)
        })
        .expect("get");
    assert!(!got && !dest.exists());
    assert!(reached > 0);
    let mut first = None;
    assert!(
        store
            .get_file("blobs/ab/abc.flac", &dest, &mut |n| {
                first.get_or_insert(n);
                true
            })
            .expect("get")
    );
    assert_eq!(first, Some(reached), "resumed, not restarted");
    assert_eq!(std::fs::read(&dest).expect("read"), song);
}

fn replicas_sync(store: &dyn Store) {
    let mut a = Replica::new(DeviceId::new());
    let mut b = Replica::new(DeviceId::new());
    let track = TrackKey::new("abc");
    a.record(
        Change::SetField {
            track: track.clone(),
            field: "title".into(),
            values: vec!["Over the network".into()],
        },
        1,
    );
    a.sync(store, Some(("laptop", "linux")), 1, |_| ())
        .expect("sync");
    let mut seen = Vec::new();
    b.sync(store, Some(("phone", "android")), 2, |e| seen.push(e))
        .expect("sync");
    assert_eq!(seen.len(), 1);
    assert_eq!(
        b.state().field(&track, "title"),
        Some(&["Over the network".to_owned()][..])
    );
    assert_eq!(store.list("devices").expect("list").len(), 2);
}

fn run(name: &str, store: Option<RemoteStore>) {
    let Some(store) = store else {
        eprintln!("skipped: no {name} server given (see the top of this file)");
        return;
    };
    let scratch = tempfile::tempdir().expect("tempdir");
    contract(&store, scratch.path());
    replicas_sync(&store);
}

#[test]
fn webdav_store() {
    run("WebDAV", webdav());
}

#[test]
fn s3_store() {
    run("S3", s3());
}

#[test]
fn encrypted_over_webdav_and_s3() {
    for (name, store) in [("WebDAV", webdav()), ("S3", s3())] {
        let Some(store) = store else {
            eprintln!("skipped: no {name} server given");
            continue;
        };
        let scratch = tempfile::tempdir().expect("tempdir");
        let (key, _) = Key::create_with("correct horse battery", 256, 1).expect("key");
        let enc = EncryptedStore::new(store, key, scratch.path().join("tmp"));
        contract(&enc, scratch.path());
        replicas_sync(&enc);
    }
}
