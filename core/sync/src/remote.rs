//! Stores on a server, through OpenDAL: WebDAV (Nextcloud, ownCloud, most NAS boxes) and
//! S3-compatible object storage (AWS, Backblaze B2, MinIO, Wasabi...). Calls block the calling
//! thread (the sync thread) on a small shared runtime. Failed requests are retried with
//! exponential backoff; an optional bandwidth cap paces song transfers.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use opendal::layers::{RetryLayer, TimeoutLayer};
use opendal::{ErrorKind, Operator, services};
use serde::{Deserialize, Serialize};

use crate::store::{Store, part_path};
use crate::{Error, Result};

static RUNTIME: LazyLock<std::result::Result<tokio::runtime::Runtime, String>> =
    LazyLock::new(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("cadenza-remote")
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
    });

const CHUNK: usize = 8 << 20;

/// Where a remote store is. Secrets (password, secret key) are kept apart from this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Remote {
    /// `url` is the folder's address, e.g. a Nextcloud
    /// `https://cloud.example.com/remote.php/dav/files/me/Cadenza`.
    WebDav { url: String, username: String },
    S3 {
        /// e.g. `https://s3.eu-central-003.backblazeb2.com`; empty for AWS.
        endpoint: String,
        bucket: String,
        region: String,
        access_key_id: String,
        /// A folder inside the bucket.
        #[serde(default)]
        root: String,
    },
}

impl Remote {
    /// For showing which store is in use.
    pub fn describe(&self) -> String {
        match self {
            Remote::WebDav { url, username } => format!("{username} at {url}"),
            Remote::S3 {
                endpoint,
                bucket,
                root,
                ..
            } => {
                let host = if endpoint.is_empty() {
                    "AWS S3"
                } else {
                    endpoint
                };
                let root = if root.is_empty() {
                    String::new()
                } else {
                    format!("/{root}")
                };
                format!("{bucket}{root} on {host}")
            }
        }
    }
}

fn remote_err(e: opendal::Error) -> Error {
    Error::Io(std::io::Error::other(e.to_string()))
}

pub struct RemoteStore {
    op: opendal::blocking::Operator,
    /// Bytes per second for song transfers; none for no limit.
    limit: Option<u64>,
}

impl std::fmt::Debug for RemoteStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteStore").finish_non_exhaustive()
    }
}

/// Paces a transfer to a byte rate.
struct Pace {
    limit: Option<u64>,
    start: Instant,
}

impl Pace {
    fn new(limit: Option<u64>) -> Self {
        Self {
            limit,
            start: Instant::now(),
        }
    }

    fn after(&self, bytes: u64) {
        if let Some(rate) = self.limit.filter(|r| *r > 0) {
            let due = Duration::from_secs_f64(bytes as f64 / rate as f64);
            if let Some(wait) = due.checked_sub(self.start.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    }
}

impl RemoteStore {
    /// Connects (lazily: nothing is sent until the first call). `secret` is the WebDAV password
    /// or the S3 secret key; `limit_kbps` caps song transfers (KB/s).
    pub fn open(remote: &Remote, secret: &str, limit_kbps: Option<u32>) -> Result<Self> {
        Self::open_with(remote, secret, limit_kbps, true)
    }

    /// For checking a server the owner just entered: one try, and a short time limit, so a wrong
    /// address fails in seconds rather than after minutes of retries.
    pub fn open_for_check(remote: &Remote, secret: &str) -> Result<Self> {
        Self::open_with(remote, secret, None, false)
    }

    fn open_with(
        remote: &Remote,
        secret: &str,
        limit_kbps: Option<u32>,
        patient: bool,
    ) -> Result<Self> {
        let runtime = RUNTIME
            .as_ref()
            .map_err(|e| Error::Io(std::io::Error::other(e.clone())))?;
        // HTTP through reqwest with rustls (first install wins, so repeating it is harmless).
        opendal::install_default();
        let op = match remote {
            Remote::WebDav { url, username } => {
                let (endpoint, root) = split_url(url)?;
                let mut b = services::Webdav::default().endpoint(&endpoint).root(&root);
                if !username.is_empty() {
                    b = b.username(username).password(secret);
                }
                Operator::new(b).map_err(remote_err)?
            }
            Remote::S3 {
                endpoint,
                bucket,
                region,
                access_key_id,
                root,
            } => {
                let mut b = services::S3::default()
                    .bucket(bucket)
                    .region(if region.is_empty() {
                        "us-east-1"
                    } else {
                        region
                    })
                    .access_key_id(access_key_id)
                    .secret_access_key(secret)
                    .root(&format!("/{}", root.trim_matches('/')))
                    .disable_config_load()
                    .disable_ec2_metadata();
                if !endpoint.is_empty() {
                    b = b.endpoint(endpoint);
                }
                Operator::new(b).map_err(remote_err)?
            }
        };
        let op = if patient {
            op.layer(TimeoutLayer::new().with_timeout(Duration::from_secs(60)))
                .layer(
                    RetryLayer::new()
                        .with_jitter()
                        .with_max_times(5)
                        .with_min_delay(Duration::from_millis(500))
                        .with_max_delay(Duration::from_secs(30)),
                )
        } else {
            op.layer(TimeoutLayer::new().with_timeout(Duration::from_secs(15)))
        };
        let _guard = runtime.enter();
        let op = opendal::blocking::Operator::new(op).map_err(remote_err)?;
        Ok(Self {
            op,
            limit: limit_kbps.map(|k| u64::from(k) * 1024),
        })
    }
}

/// Reads until `buf` is full or the file ends (S3 parts must be full-size but the last).
fn read_full(src: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match src.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// `https://host/a/b` → (`https://host`, `/a/b/`).
fn split_url(url: &str) -> Result<(String, String)> {
    let bad = || Error::Path(format!("{url} isn't a web address (https://…)"));
    let rest = url
        .strip_prefix("https://")
        .map(|r| ("https://", r))
        .or_else(|| url.strip_prefix("http://").map(|r| ("http://", r)))
        .ok_or_else(bad)?;
    let (scheme, rest) = rest;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host.is_empty() {
        return Err(bad());
    }
    let path = path.trim_matches('/');
    let root = if path.is_empty() {
        "/".to_owned()
    } else {
        format!("/{path}/")
    };
    Ok((format!("{scheme}{host}"), root))
}

/// Runs `f` where it may block: on this thread, or on a fresh one when this thread belongs to an
/// async runtime (an app shell's command handler), where blocking on our runtime would panic.
fn off_runtime<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    if tokio::runtime::Handle::try_current().is_err() {
        return f();
    }
    std::thread::scope(|s| match s.spawn(f).join() {
        Ok(v) => v,
        Err(panic) => std::panic::resume_unwind(panic),
    })
}

impl Store for RemoteStore {
    fn list(&self, dir: &str) -> Result<Vec<String>> {
        off_runtime(|| self.list_here(dir))
    }
    fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        off_runtime(|| self.get_here(path))
    }
    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        off_runtime(|| self.put_here(path, bytes))
    }
    fn size(&self, path: &str) -> Result<Option<u64>> {
        off_runtime(|| self.size_here(path))
    }
    fn put_file(&self, path: &str, local: &Path, go: &mut dyn FnMut(u64) -> bool) -> Result<bool> {
        if tokio::runtime::Handle::try_current().is_err() {
            return self.put_file_here(path, local, go);
        }
        // `go` needn't be Send; report through a channel instead.
        let (tx, rx) = std::sync::mpsc::channel::<(u64, std::sync::mpsc::Sender<bool>)>();
        std::thread::scope(|s| {
            let worker = s.spawn(move || {
                self.put_file_here(path, local, &mut |n| {
                    let (back, answer) = std::sync::mpsc::channel();
                    tx.send((n, back)).is_ok() && answer.recv().unwrap_or(false)
                })
            });
            for (n, back) in rx {
                let _ = back.send(go(n));
            }
            worker
                .join()
                .unwrap_or_else(|p| std::panic::resume_unwind(p))
        })
    }
    fn get_file(&self, path: &str, dest: &Path, go: &mut dyn FnMut(u64) -> bool) -> Result<bool> {
        if tokio::runtime::Handle::try_current().is_err() {
            return self.get_file_here(path, dest, go);
        }
        let (tx, rx) = std::sync::mpsc::channel::<(u64, std::sync::mpsc::Sender<bool>)>();
        std::thread::scope(|s| {
            let worker = s.spawn(move || {
                self.get_file_here(path, dest, &mut |n| {
                    let (back, answer) = std::sync::mpsc::channel();
                    tx.send((n, back)).is_ok() && answer.recv().unwrap_or(false)
                })
            });
            for (n, back) in rx {
                let _ = back.send(go(n));
            }
            worker
                .join()
                .unwrap_or_else(|p| std::panic::resume_unwind(p))
        })
    }
}

impl RemoteStore {
    fn list_here(&self, dir: &str) -> Result<Vec<String>> {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        let entries = match self.op.list(&prefix) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(remote_err(e)),
        };
        // WebDAV lists the folder itself too.
        let mut names: Vec<String> = entries
            .iter()
            .filter(|e| e.path().trim_start_matches('/') != prefix)
            .map(|e| e.name().trim_end_matches('/').to_owned())
            .filter(|n| !n.is_empty() && !n.starts_with('.'))
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn get_here(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match self.op.read(path) {
            Ok(b) => Ok(Some(b.to_vec())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(remote_err(e)),
        }
    }

    fn put_here(&self, path: &str, bytes: &[u8]) -> Result<()> {
        self.op.write(path, bytes.to_vec()).map_err(remote_err)?;
        Ok(())
    }

    fn size_here(&self, path: &str) -> Result<Option<u64>> {
        match self.op.stat(path) {
            Ok(m) => Ok(Some(m.content_length())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(remote_err(e)),
        }
    }

    fn put_file_here(
        &self,
        path: &str,
        local: &Path,
        go: &mut dyn FnMut(u64) -> bool,
    ) -> Result<bool> {
        // WebDAV takes a file in one request; S3 in parts (multipart upload).
        if !self.op.info().capability().write_can_multi {
            if !go(0) {
                return Ok(false);
            }
            let bytes = fs::read(local)?;
            let n = bytes.len() as u64;
            self.op.write(path, bytes).map_err(remote_err)?;
            Pace::new(self.limit).after(n);
            go(n);
            return Ok(true);
        }
        let mut src = fs::File::open(local)?;
        let mut writer = self.op.writer(path).map_err(remote_err)?;
        let pace = Pace::new(self.limit);
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        loop {
            if !go(done) {
                // Dropped without closing: the write is never committed, so nothing appears at
                // `path`.
                drop(writer);
                return Ok(false);
            }
            let n = read_full(&mut src, &mut buf)?;
            if n == 0 {
                break;
            }
            writer.write(buf[..n].to_vec()).map_err(remote_err)?;
            done += n as u64;
            pace.after(done);
        }
        writer.close().map_err(remote_err)?;
        go(done);
        Ok(true)
    }

    fn get_file_here(
        &self,
        path: &str,
        dest: &Path,
        go: &mut dyn FnMut(u64) -> bool,
    ) -> Result<bool> {
        let total = self
            .size(path)?
            .ok_or_else(|| Error::Path(format!("{path} isn't in the store")))?;
        let part = part_path(dest)?;
        // Objects never change once written, so a partial download carries on where it stopped.
        let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let (mut out, mut done) = if have > 0 && have <= total {
            (fs::OpenOptions::new().append(true).open(&part)?, have)
        } else {
            (fs::File::create(&part)?, 0)
        };
        let reader = self.op.reader(path).map_err(remote_err)?;
        let pace = Pace::new(self.limit);
        let resumed_at = done;
        while done < total {
            if !go(done) {
                out.flush()?;
                return Ok(false);
            }
            let end = (done + CHUNK as u64).min(total);
            let chunk = reader.read(done..end).map_err(remote_err)?;
            let bytes = chunk.to_vec();
            if bytes.is_empty() {
                break;
            }
            out.write_all(&bytes)?;
            done += bytes.len() as u64;
            pace.after(done - resumed_at);
        }
        out.sync_all()?;
        drop(out);
        if fs::metadata(&part)?.len() != total {
            let _ = fs::remove_file(&part);
            return Err(Error::Segment(format!("{path} changed while downloading")));
        }
        fs::rename(&part, dest)?;
        go(total);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_addresses_split_into_server_and_folder() {
        assert_eq!(
            split_url("https://cloud.example.com/remote.php/dav/files/me/Cadenza/").expect("url"),
            (
                "https://cloud.example.com".to_owned(),
                "/remote.php/dav/files/me/Cadenza/".to_owned()
            )
        );
        assert_eq!(
            split_url("http://nas:8080").expect("url"),
            ("http://nas:8080".to_owned(), "/".to_owned())
        );
        assert!(split_url("ftp://x").is_err());
        assert!(split_url("https:///x").is_err());
    }

    #[test]
    fn pacing_waits_for_the_byte_rate() {
        let pace = Pace::new(Some(1_000_000));
        let start = Instant::now();
        pace.after(100_000);
        assert!(start.elapsed() >= Duration::from_millis(90));
        let free = Pace::new(None);
        let start = Instant::now();
        free.after(u64::MAX / 2);
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}
