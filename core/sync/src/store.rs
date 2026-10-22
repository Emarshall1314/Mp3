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
}
