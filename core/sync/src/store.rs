//! Where the shared logs live. Every object is written whole and never changed afterwards, so a
//! store only needs to list, read and write files; that suits a plain folder kept in step by
//! another tool (Syncthing, Nextcloud, a USB drive) as well as cloud drives.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use crate::{Error, Result};

pub trait Store: Send + Sync {
    /// The names directly inside `dir` ("log", "log/<device>"), sorted; empty if it isn't there.
    fn list(&self, dir: &str) -> Result<Vec<String>>;
    /// The object at `path`, or none.
    fn get(&self, path: &str) -> Result<Option<Vec<u8>>>;
    /// Writes the object at `path` whole: readers see the old object or the new one, never part.
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()>;

    /// The object's size, or none if it isn't there.
    fn size(&self, path: &str) -> Result<Option<u64>> {
        Ok(self.get(path)?.map(|b| b.len() as u64))
    }

    /// Uploads a local file. `go` hears the bytes sent so far and returns false to stop
    /// (pause); the result is then false and the object isn't written.
    fn put_file(&self, path: &str, local: &Path, go: &mut dyn FnMut(u64) -> bool) -> Result<bool> {
        let bytes = fs::read(local)?;
        if !go(0) {
            return Ok(false);
        }
        self.put(path, &bytes)?;
        go(bytes.len() as u64);
        Ok(true)
    }

    /// Downloads an object to `dest` through a hidden `.part` file beside it, renamed into place
    /// when complete, so a partial download is never seen as a song. Stopping (`go` returning
    /// false) keeps the `.part` so the next download resumes where it stopped. False if
    /// stopped; an error if the object isn't there.
    fn get_file(&self, path: &str, dest: &Path, go: &mut dyn FnMut(u64) -> bool) -> Result<bool> {
        let bytes = self
            .get(path)?
            .ok_or_else(|| Error::Path(format!("{path} isn't in the store")))?;
        if !go(0) {
            return Ok(false);
        }
        let part = part_path(dest)?;
        fs::write(&part, &bytes)?;
        fs::rename(&part, dest)?;
        go(bytes.len() as u64);
        Ok(true)
    }
}

/// The hidden file a download goes to first.
pub fn part_path(dest: &Path) -> Result<PathBuf> {
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Path(dest.display().to_string()))?;
    Ok(dest.with_file_name(format!(".{name}.part")))
}

const CHUNK: usize = 1 << 20;

/// Copies `from` (starting at `skip`) onto the end of `to`, reporting the total so far.
fn copy(
    from: &Path,
    skip: u64,
    to: &mut fs::File,
    go: &mut dyn FnMut(u64) -> bool,
) -> Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut src = fs::File::open(from)?;
    src.seek(SeekFrom::Start(skip))?;
    let mut done = skip;
    let mut buf = vec![0; CHUNK];
    loop {
        if !go(done) {
            to.flush()?;
            return Ok(false);
        }
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        to.write_all(&buf[..n])?;
        done += n as u64;
    }
    to.sync_all()?;
    go(done);
    Ok(true)
}

/// Store paths are relative, with `/` between parts, and never leave the store.
fn check(path: &str) -> Result<()> {
    let ok = !path.is_empty()
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains('\\'));
    if ok {
        Ok(())
    } else {
        Err(Error::Path(path.into()))
    }
}

/// A store in a folder on this device.
#[derive(Debug, Clone)]
pub struct FolderStore {
    root: PathBuf,
}

impl FolderStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, path: &str) -> Result<PathBuf> {
        check(path)?;
        let full = self.root.join(path);
        debug_assert!(full.components().all(|c| c != Component::ParentDir));
        Ok(full)
    }
}

impl Store for FolderStore {
    fn list(&self, dir: &str) -> Result<Vec<String>> {
        let entries = match fs::read_dir(self.path(dir)?) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut names = Vec::new();
        for entry in entries {
            let name = entry?.file_name();
            // Hidden names are other tools' temporary files (and our own while writing):
            // Syncthing's `.syncthing.*.tmp`, Nextcloud's `.~lock`, our `.*.part`.
            if let Some(name) = name.to_str().filter(|n| !n.starts_with('.')) {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match fs::read(self.path(path)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let full = self.path(path)?;
        let dir = full.parent().ok_or_else(|| Error::Path(path.into()))?;
        fs::create_dir_all(dir)?;
        let name = full
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::Path(path.into()))?;
        let temp = dir.join(format!(".{name}.part"));
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &full)?;
        // The rename is only durable once the folder is synced too.
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    }

    fn size(&self, path: &str) -> Result<Option<u64>> {
        match fs::metadata(self.path(path)?) {
            Ok(m) => Ok(Some(m.len())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn put_file(&self, path: &str, local: &Path, go: &mut dyn FnMut(u64) -> bool) -> Result<bool> {
        let full = self.path(path)?;
        let dir = full.parent().ok_or_else(|| Error::Path(path.into()))?;
        fs::create_dir_all(dir)?;
        // Uploads start again rather than resume: the local file's tags may have changed since.
        let part = part_path(&full)?;
        let mut out = fs::File::create(&part)?;
        if !copy(local, 0, &mut out, go)? {
            drop(out);
            let _ = fs::remove_file(&part);
            return Ok(false);
        }
        fs::rename(&part, &full)?;
        Ok(true)
    }

    fn get_file(&self, path: &str, dest: &Path, go: &mut dyn FnMut(u64) -> bool) -> Result<bool> {
        let from = self.path(path)?;
        let total = fs::metadata(&from)
            .map_err(|_| Error::Path(format!("{path} isn't in the store")))?
            .len();
        let part = part_path(dest)?;
        // Objects in the store never change, so a partial download can be continued.
        let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let mut out = if have > 0 && have <= total {
            fs::OpenOptions::new().append(true).open(&part)?
        } else {
            fs::File::create(&part)?
        };
        let skip = if have <= total { have } else { 0 };
        if !copy(&from, skip, &mut out, go)? {
            return Ok(false);
        }
        drop(out);
        if fs::metadata(&part)?.len() != total {
            let _ = fs::remove_file(&part);
            return Err(Error::Segment(format!("{path} changed while downloading")));
        }
        fs::rename(&part, dest)?;
        Ok(true)
    }
}

/// A store in memory, for tests.
#[derive(Debug, Default)]
pub struct MemStore {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn objects(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<u8>>> {
        self.objects.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Store for MemStore {
    fn list(&self, dir: &str) -> Result<Vec<String>> {
        check(dir)?;
        let prefix = format!("{dir}/");
        let mut names: Vec<String> = self
            .objects()
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .map(|rest| rest.split('/').next().unwrap_or(rest).to_owned())
            .collect();
        names.dedup();
        Ok(names)
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        check(path)?;
        Ok(self.objects().get(path).cloned())
    }

    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        check(path)?;
        self.objects().insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(store: &dyn Store) {
        assert_eq!(store.list("log").expect("list"), Vec::<String>::new());
        assert_eq!(store.get("log/a/1.seg").expect("get"), None);
        store.put("log/b/2.seg", b"two").expect("put");
        store.put("log/a/1.seg", b"one").expect("put");
        store.put("log/a/0.seg", b"zero").expect("put");
        assert_eq!(store.list("log").expect("list"), ["a", "b"]);
        assert_eq!(store.list("log/a").expect("list"), ["0.seg", "1.seg"]);
        assert_eq!(
            store.get("log/a/1.seg").expect("get").as_deref(),
            Some(&b"one"[..])
        );
        for bad in ["", "/etc/passwd", "../x", "log/../../x", "log//a", "a\\b"] {
            assert!(store.put(bad, b"x").is_err(), "{bad}");
        }
    }

    #[test]
    fn folder_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FolderStore::new(dir.path());
        exercise(&store);
        // Other tools' temporary files are ignored, and ours don't linger.
        fs::write(dir.path().join("log/a/.syncthing.x.tmp"), b"").expect("write");
        assert_eq!(store.list("log/a").expect("list"), ["0.seg", "1.seg"]);
        assert!(!dir.path().join("log/a/.1.seg.part").exists());
    }

    #[test]
    fn mem_store() {
        exercise(&MemStore::new());
    }

    #[test]
    fn interrupted_downloads_resume_and_are_never_seen_half_done() {
        let store_dir = tempfile::tempdir().expect("tempdir");
        let local = tempfile::tempdir().expect("tempdir");
        let store = FolderStore::new(store_dir.path());
        let song: Vec<u8> = (0..(CHUNK * 3 + 123)).map(|i| (i % 251) as u8).collect();
        let src = local.path().join("song.flac");
        fs::write(&src, &song).expect("write");

        // An upload stopped part-way leaves nothing behind.
        let mut calls = 0;
        let done = store
            .put_file("blobs/ab/abc.flac", &src, &mut |_| {
                calls += 1;
                calls < 2
            })
            .expect("put");
        assert!(!done);
        assert_eq!(store.size("blobs/ab/abc.flac").expect("size"), None);
        assert!(
            store
                .put_file("blobs/ab/abc.flac", &src, &mut |_| true)
                .expect("put")
        );
        assert_eq!(
            store.size("blobs/ab/abc.flac").expect("size"),
            Some(song.len() as u64)
        );

        // A download stopped after a chunk keeps its .part, hidden; the next one carries on.
        let dest = local.path().join("copy.flac");
        let mut seen = Vec::new();
        let done = store
            .get_file("blobs/ab/abc.flac", &dest, &mut |n| {
                seen.push(n);
                n < CHUNK as u64
            })
            .expect("get");
        assert!(!done && !dest.exists());
        assert_eq!(
            fs::metadata(part_path(&dest).expect("part"))
                .expect("part")
                .len(),
            CHUNK as u64
        );
        let mut first = None;
        assert!(
            store
                .get_file("blobs/ab/abc.flac", &dest, &mut |n| {
                    first.get_or_insert(n);
                    true
                })
                .expect("get")
        );
        assert_eq!(first, Some(CHUNK as u64), "resumed, not restarted");
        assert_eq!(fs::read(&dest).expect("read"), song);
        assert!(!part_path(&dest).expect("part").exists());
        assert!(store.get_file("blobs/none", &dest, &mut |_| true).is_err());
    }
}
