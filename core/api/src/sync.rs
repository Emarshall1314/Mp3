//! Sync in the app (D55, D56, D64): turns this device's tag edits, favourites, listens and trash
//! into ops, and applies other devices' ops to this device's own files and catalogue.
//!
//! Devices sync over the Wi-Fi they share. Everything lives in `<data>/sync/`: `config.json`
//! (this device's name, sharing options, addresses typed in), `library.json` (the library this
//! device is paired into: the public part of its key derivation), `key` and `code` (the library
//! key and the pairing code, readable only by this user), `wifi.cbor` (the merged state, what has
//! been applied here, the outbox) and `store/` (this device's log, the copies of the others',
//! device and handoff files, artwork).
//!
//! A background thread syncs a minute after the last sync, a few seconds after a local change, or
//! when asked (by the owner, or by another device with news). Each round: copy what the paired
//! devices in reach have that this one lacks, push this device's ops and read the others', then
//! apply whatever hasn't been applied here yet. Applying is driven by the merged state, not by
//! the ops as they arrive, so a change for a song this device doesn't have yet applies when the
//! song turns up, and a change that couldn't be applied (the tag writer was busy) is tried again
//! next round.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use cadenza_library::Library;
use cadenza_sync::crypt::{normalize_code, pairing_code};
use cadenza_sync::discovery::Discovery;
use cadenza_sync::hlc::wall_ms;
use cadenza_sync::peer::{self, Hello, Host, Me, Peer, Server};
use cadenza_sync::replica::DeviceInfo;
use cadenza_sync::{
    Change, DeviceId, Effect, Encryption, FolderStore, Hlc, Key, Replica, Store, TrackKey,
};
use serde::{Deserialize, Serialize};

use crate::edit::{Editor, PictureEdit, TrackEdit, parse_picture};
use crate::{ApiEvent, Emit, Error, Result, Scanner};

/// How often to sync when nothing asks sooner.
const INTERVAL: Duration = Duration::from_secs(60);
/// How long to wait after a local change, so a burst of changes goes in one segment.
const SETTLE: Duration = Duration::from_secs(3);
/// How long a device that contacted this one is tried again without being found otherwise.
const SEEN_FOR: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncSettings {
    /// How other devices see this one.
    pub device_name: String,
    /// Let devices that don't have this device's songs copy them from it.
    pub share_audio: bool,
    /// Copy songs this device doesn't have from devices that share them, into its first music
    /// folder.
    pub download_audio: bool,
    /// Stop syncing until turned back on.
    pub paused: bool,
    /// Devices' addresses typed in ("192.168.1.20" or "192.168.1.20:47100"), for networks where
    /// devices can't find each other by themselves.
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDevice {
    pub id: String,
    pub name: String,
    pub platform: String,
    /// Milliseconds since the Unix epoch.
    pub last_push_ms: u64,
    pub this_device: bool,
    /// Reached in the last round.
    pub online: bool,
}

/// Another Cadenza on this network.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyDevice {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub address: String,
    /// It syncs with this device already.
    pub paired: bool,
    /// It's syncing (with this device or others), so this device can join it.
    pub syncing: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    /// "upload" or "download".
    pub direction: String,
    pub name: String,
    pub done: u64,
    pub total: u64,
    /// Files still to go after this one.
    pub remaining: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub settings: SyncSettings,
    pub device_id: Option<String>,
    /// In a library: syncing with the devices paired into it.
    pub paired: bool,
    /// The code another device types to join, shown on every device in the library.
    pub pairing_code: Option<String>,
    /// Where this device can be reached on the network, for typing into another.
    pub addresses: Vec<String>,
    /// The port this device listens on for the others (0 if it couldn't open one).
    pub port: u16,
    pub syncing: bool,
    pub last_sync_ms: Option<u64>,
    pub error: Option<String>,
    /// Changes made here that no other device has copied yet.
    pub pending: usize,
    pub devices: Vec<SyncDevice>,
    pub conflicts: usize,
    pub transfer: Option<Transfer>,
    /// Things that couldn't be synced and why (one line each, most recent round).
    pub problems: Vec<String>,
    /// What other devices were last playing, newest first.
    pub handoffs: Vec<Handoff>,
}

fn other(e: cadenza_sync::Error) -> Error {
    Error::Other(e.to_string())
}

/// An edit that lost to a concurrent edit on another device, for the owner to review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConflict {
    pub audio_hash: String,
    /// The song's title and artist here, if this device has it.
    pub title: Option<String>,
    pub artist: Option<String>,
    /// A tag field, or "favourite" or "trash".
    pub field: String,
    pub kept: Vec<String>,
    pub kept_device: String,
    pub kept_at_ms: u64,
    pub lost: Vec<String>,
    pub lost_device: String,
    pub lost_at_ms: u64,
}

/// What a device was last playing, as it publishes it for the others (`handoff/<device>.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HandoffFile {
    device: String,
    name: String,
    /// When published, ms since the Unix epoch.
    at_ms: u64,
    /// Audio hashes in play order.
    queue: Vec<String>,
    index: usize,
    position_ms: u64,
    title: String,
    artist: String,
}

/// Another device's listening, to carry on here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Handoff {
    pub device_id: String,
    pub device_name: String,
    pub at_ms: u64,
    pub title: String,
    pub artist: String,
    pub position_ms: u64,
    /// Songs in that queue, and how many of them this device has.
    pub songs: usize,
    pub here: usize,
}

/// A handoff turned into this device's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resume {
    pub ids: Vec<i64>,
    pub index: usize,
    pub position_ms: u64,
}

/// How long a device's listening stays on offer to the others.
const HANDOFF_DAYS: u64 = 14;
/// At most this many songs of a queue are published (around the playing one).
const HANDOFF_SONGS: usize = 1000;

/// What was last published for handoff: queue version, index, position in 10 s steps, playing.
type PublishMark = (u64, Option<usize>, u64, bool);

/// What `wifi.cbor` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Saved {
    replica: Replica,
    /// For each value, the version applied to this device's files and catalogue.
    applied: BTreeMap<(TrackKey, String), Hlc>,
    /// This device's favourites from before sync was on have been shared.
    exported_favourites: bool,
    /// Songs scanned since then get their synced listens (seconds).
    scan_check: i64,
}

impl Saved {
    fn new() -> Self {
        Self {
            replica: Replica::new(DeviceId::new()),
            applied: BTreeMap::new(),
            exported_favourites: false,
            scan_check: 0,
        }
    }
}

struct Inner {
    settings: SyncSettings,
    saved: Option<Saved>,
    /// The library this device is in, with its key and pairing code.
    library: Option<(Encryption, Key, String)>,
}

pub(crate) struct Syncer {
    library: Arc<Library>,
    editor: Arc<Editor>,
    scanner: Arc<Scanner>,
    emit: Emit,
    dir: PathBuf,
    inner: Mutex<Inner>,
    status: Mutex<SyncStatus>,
    wake: Mutex<Option<Sender<()>>>,
    /// Stops the transfer in progress (pause, or settings changed).
    stop: AtomicBool,
    /// One round at a time.
    round_lock: Mutex<()>,
    /// Back up files before applying tag changes (the tag-backups setting).
    pub(crate) backups: AtomicBool,
    /// The player, for handoff.
    player: cadenza_engine::Observer,
    /// The last listening published.
    published: Mutex<Option<PublishMark>>,
    /// Other devices' listening as last read.
    handoffs: Mutex<Vec<HandoffFile>>,
    /// Answers the other devices; none if no port could be opened.
    server: Mutex<Option<Server>>,
    /// Finds the other devices; none where mDNS can't run (or in tests).
    discovery: Option<Discovery>,
    /// Paired devices that contacted this one, and where from.
    seen: Mutex<HashMap<String, (SocketAddr, Instant)>>,
}

fn platform() -> String {
    if cfg!(target_os = "android") {
        "android".into()
    } else {
        std::env::consts::OS.into()
    }
}

/// This machine's name, for a first guess at the device name.
fn host_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "This computer".into())
}

/// Audio hashes are hex (with `:n` for a cue song); anything else never becomes a store path.
fn safe_hash(hash: &str) -> bool {
    !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit() || b == b':')
}

fn blob_path(hash: &str, ext: &str) -> Option<String> {
    let ext_ok =
        !ext.is_empty() && ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric());
    (safe_hash(hash) && ext_ok && hash.len() > 2)
        .then(|| format!("blobs/{}/{hash}.{ext}", &hash[..2]))
}

/// The audio hash in a `blobs/<aa>/<hash>.<ext>` path.
fn blob_hash(path: &str) -> Option<&str> {
    let name = path.strip_prefix("blobs/")?.split('/').nth(1)?;
    let hash = name.rsplit_once('.')?.0;
    safe_hash(hash).then_some(hash)
}

fn art_path(hash: &str) -> Option<String> {
    (safe_hash(hash) && hash.len() > 2).then(|| format!("art/{}/{hash}", &hash[..2]))
}

/// A relative path from another device, if it stays inside the folder it's joined to.
fn safe_relative(rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    (!rel.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_))))
        .then(|| p.to_owned())
}

fn ms_to_s(ms: u64) -> i64 {
    (ms / 1000) as i64
}

/// The server's view of the syncer.
struct Serving(Weak<Syncer>);

impl Host for Serving {
    fn hello(&self) -> Hello {
        match self.0.upgrade() {
            Some(s) => s.hello(),
            None => Hello {
                device: String::new(),
                name: String::new(),
                platform: platform(),
                library: None,
            },
        }
    }

    fn key(&self) -> Option<Key> {
        let s = self.0.upgrade()?;
        let inner = s.inner();
        inner.saved.as_ref()?;
        inner.library.as_ref().map(|(_, k, _)| k.clone())
    }

    fn store_root(&self) -> PathBuf {
        self.0.upgrade().map(|s| s.store_dir()).unwrap_or_default()
    }

    fn blob(&self, path: &str) -> Option<PathBuf> {
        let s = self.0.upgrade()?;
        if !s.inner().settings.share_audio {
            return None;
        }
        s.library.file_with_hash(blob_hash(path)?).ok().flatten()
    }

    fn seen(&self, device: &str, at: SocketAddr, poked: bool) {
        let Some(s) = self.0.upgrade() else { return };
        if at.port() != 0
            && let Ok(mut seen) = s.seen.lock()
        {
            seen.insert(device.to_owned(), (at, Instant::now()));
        }
        if poked {
            s.wake();
        }
    }
}

/// Where to look for a paired device: its id if known, and addresses to try in turn.
type Target = (Option<String>, Vec<SocketAddr>);

impl Syncer {
    pub(crate) fn new(
        library: Arc<Library>,
        editor: Arc<Editor>,
        scanner: Arc<Scanner>,
        emit: Emit,
        data_dir: &Path,
        player: cadenza_engine::Observer,
    ) -> Arc<Self> {
        let dir = data_dir.join("sync");
        let mut settings: SyncSettings = std::fs::read(dir.join("config.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if settings.device_name.trim().is_empty() {
            settings.device_name = host_name();
        }
        let library_info = (|| {
            let e: Encryption =
                serde_json::from_slice(&std::fs::read(dir.join("library.json")).ok()?).ok()?;
            let key = Key::from_bytes(&std::fs::read(dir.join("key")).ok()?)?;
            let code = String::from_utf8(std::fs::read(dir.join("code")).ok()?).ok()?;
            Some((e, key, code))
        })();
        let mut saved = std::fs::read(dir.join("wifi.cbor"))
            .ok()
            .and_then(|b| ciborium::from_reader::<Saved, _>(b.as_slice()).ok());
        // In a library but no state (the state file was lost): start again as a new device,
        // which copies everything from the others.
        if library_info.is_some() && saved.is_none() {
            saved = Some(Saved::new());
        }
        let discovery = if std::env::var_os("CADENZA_NO_DISCOVERY").is_some() {
            None
        } else {
            Discovery::start().ok()
        };
        let me = Arc::new(Self {
            library,
            editor,
            scanner,
            emit,
            dir,
            inner: Mutex::new(Inner {
                settings,
                saved,
                library: library_info,
            }),
            status: Mutex::new(SyncStatus::default()),
            wake: Mutex::new(None),
            stop: AtomicBool::new(false),
            round_lock: Mutex::new(()),
            backups: AtomicBool::new(false),
            player,
            published: Mutex::new(None),
            handoffs: Mutex::new(Vec::new()),
            server: Mutex::new(None),
            discovery,
            seen: Mutex::new(HashMap::new()),
        });
        let server = Server::start(Arc::new(Serving(Arc::downgrade(&me))));
        if let Ok(mut slot) = me.server.lock() {
            *slot = server.ok();
        }
        me.advertise();
        me.refresh_status();
        let (tx, rx) = channel();
        if let Ok(mut w) = me.wake.lock() {
            *w = Some(tx);
        }
        let weak = Arc::downgrade(&me);
        let _ = thread::Builder::new()
            .name("cadenza-sync".into())
            .spawn(move || run(weak, rx));
        me.wake();
        me
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wake(&self) {
        if let Ok(w) = self.wake.lock()
            && let Some(tx) = w.as_ref()
        {
            let _ = tx.send(());
        }
    }

    fn store_dir(&self) -> PathBuf {
        self.dir.join("store")
    }

    fn store(&self) -> FolderStore {
        FolderStore::new(self.store_dir())
    }

    /// The port this device's server listens on (0 if none).
    pub(crate) fn port(&self) -> u16 {
        self.server
            .lock()
            .ok()
            .and_then(|s| s.as_ref().map(Server::port))
            .unwrap_or(0)
    }

    fn device_id(&self) -> Option<String> {
        let inner = self.inner();
        inner.saved.as_ref().map(|s| s.replica.device().to_string())
    }

    fn hello(&self) -> Hello {
        let inner = self.inner();
        Hello {
            device: inner
                .saved
                .as_ref()
                .map(|s| s.replica.device().to_string())
                .unwrap_or_default(),
            name: inner.settings.device_name.clone(),
            platform: platform(),
            library: inner
                .saved
                .as_ref()
                .and(inner.library.as_ref())
                .map(|(e, _, _)| e.clone()),
        }
    }

    /// Tells the network who this device is now.
    fn advertise(&self) {
        let port = self.port();
        if let Some(d) = &self.discovery
            && port != 0
        {
            let mut hello = self.hello();
            if hello.device.is_empty() {
                // Not syncing yet: still visible (so it can be chosen to join), under a
                // throwaway id.
                hello.device = format!("new-{}", DeviceId::new());
            }
            let _ = d.advertise(&hello, port);
        }
    }

    fn save(&self, inner: &Inner) -> Result<()> {
        let io = |e: std::io::Error| Error::Other(format!("saving sync state: {e}"));
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let json =
            serde_json::to_vec_pretty(&inner.settings).map_err(|e| Error::Other(e.to_string()))?;
        write_atomic(&self.dir.join("config.json"), &json).map_err(io)?;
        if let Some(saved) = &inner.saved {
            let mut bytes = Vec::new();
            ciborium::into_writer(saved, &mut bytes).map_err(|e| Error::Other(e.to_string()))?;
            write_atomic(&self.dir.join("wifi.cbor"), &bytes).map_err(io)?;
        }
        Ok(())
    }

    /// Writes a file in the sync folder that only this user can read.
    fn write_private(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let path = self.dir.join(name);
        let io = |e: std::io::Error| Error::Other(format!("saving {name}: {e}"));
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let tmp = path.with_extension("tmp");
        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
            let mut f = opts.open(&tmp).map_err(io)?;
            std::io::Write::write_all(&mut f, bytes).map_err(io)?;
        }
        std::fs::rename(&tmp, &path).map_err(io)
    }

    // ---- settings and status ------------------------------------------------------------

    pub(crate) fn status(&self) -> SyncStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn set_status(&self, f: impl FnOnce(&mut SyncStatus)) {
        let status = {
            let Ok(mut s) = self.status.lock() else {
                return;
            };
            f(&mut s);
            s.clone()
        };
        (self.emit)(ApiEvent::Sync {
            status: Box::new(status),
        });
    }

    /// Copies settings, device, pairing and counts into the status.
    fn refresh_status(&self) {
        let (settings, device, pending, conflicts, code) = {
            let inner = self.inner();
            let saved = inner.saved.as_ref();
            (
                inner.settings.clone(),
                saved.map(|s| s.replica.device().to_string()),
                saved.map_or(0, |s| s.replica.pending()),
                saved.map_or(0, |s| s.replica.state().conflicts().len()),
                saved.and(inner.library.as_ref()).map(|(_, _, c)| c.clone()),
            )
        };
        let port = self.port();
        let addresses = if port == 0 {
            Vec::new()
        } else {
            local_addresses()
                .into_iter()
                .map(|ip| SocketAddr::new(ip, port).to_string())
                .collect()
        };
        self.set_status(|s| {
            s.settings = settings;
            s.paired = code.is_some();
            s.device_id = device;
            s.pending = pending;
            s.conflicts = conflicts;
            s.pairing_code = code;
            s.addresses = addresses;
            s.port = port;
        });
    }

    pub(crate) fn set_settings(&self, mut settings: SyncSettings) -> Result<SyncStatus> {
        if settings.device_name.trim().is_empty() {
            settings.device_name = host_name();
        }
        settings.device_name = settings.device_name.trim().chars().take(60).collect();
        let mut addresses = Vec::new();
        for a in &settings.addresses {
            let a = a.trim();
            if a.is_empty() || addresses.iter().any(|b: &String| b == a) {
                continue;
            }
            if peer::parse_address(a).is_none() {
                return Err(Error::Other(format!("“{a}” isn't an address")));
            }
            addresses.push(a.to_owned());
        }
        settings.addresses = addresses;
        {
            let mut inner = self.inner();
            inner.settings = settings;
            self.save(&inner)?;
        }
        // A transfer under the old settings stops; the next round starts under the new ones.
        self.stop.store(true, Ordering::Release);
        self.advertise();
        self.refresh_status();
        self.wake();
        Ok(self.status())
    }

    /// Starts a new library with this device in it: the others join with its pairing code.
    pub(crate) fn start_library(&self) -> Result<SyncStatus> {
        if self.inner().library.is_some() {
            return Err(Error::Other("this device is already syncing".into()));
        }
        let code = pairing_code();
        let (key, encryption) = Key::create(&code).map_err(other)?;
        self.enter(encryption, key, code)
    }

    /// Joins the library of the device at `address` (or with that device id, found on the
    /// network), with the pairing code shown on it.
    pub(crate) fn join_library(&self, address: &str, code: &str) -> Result<SyncStatus> {
        if self.inner().library.is_some() {
            return Err(Error::Other(
                "this device is already syncing: stop first to join another".into(),
            ));
        }
        let code = normalize_code(code).ok_or_else(|| {
            Error::Other("a pairing code is 12 letters and digits, like ABCD-EFGH-JK23".into())
        })?;
        let found = self.discovery.as_ref().and_then(|d| {
            d.found("")
                .into_iter()
                .find(|f| f.device == address)
                .map(|f| f.addrs)
        });
        let typed = found.is_none();
        let addrs = match found {
            Some(a) => a,
            None => vec![peer::parse_address(address).ok_or_else(|| {
                Error::Other(format!("“{address}” isn't an address (like 192.168.1.20)"))
            })?],
        };
        let mut last = None;
        let mut reached = None;
        for addr in addrs {
            match peer::hello(addr) {
                Ok(h) => {
                    reached = Some((addr, h));
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        let Some((addr, hello)) = reached else {
            return Err(Error::Other(format!(
                "couldn't reach that device ({}): is Cadenza open on it, on the same Wi-Fi?",
                last.map(|e| e.to_string()).unwrap_or_default()
            )));
        };
        let encryption = hello.library.ok_or_else(|| {
            Error::Other(format!(
                "{} isn't syncing yet: choose Start Syncing there first",
                hello.name
            ))
        })?;
        let key = Key::unlock(&code, &encryption).map_err(|_| {
            Error::Other(format!(
                "that isn't the pairing code shown on {}",
                hello.name
            ))
        })?;
        if typed {
            let mut inner = self.inner();
            let a = address.trim().to_owned();
            if !inner.settings.addresses.contains(&a) {
                inner.settings.addresses.push(a);
            }
        }
        let status = self.enter(encryption, key, code)?;
        // Let it know where this device is, so it copies from here too.
        if let Some(me) = self.me() {
            let _ = Peer::new(addr, me).poke();
        }
        Ok(status)
    }

    fn enter(&self, encryption: Encryption, key: Key, code: String) -> Result<SyncStatus> {
        let json =
            serde_json::to_vec_pretty(&encryption).map_err(|e| Error::Other(e.to_string()))?;
        self.write_private("key", &key.to_bytes())?;
        self.write_private("code", code.as_bytes())?;
        write_atomic(&self.dir.join("library.json"), &json)
            .map_err(|e| Error::Other(e.to_string()))?;
        // A store from an earlier library has nothing to do with this one.
        let _ = std::fs::remove_dir_all(self.store_dir());
        {
            let mut inner = self.inner();
            inner.library = Some((encryption, key, code));
            inner.saved = Some(Saved::new());
            self.save(&inner)?;
        }
        self.advertise();
        self.refresh_status();
        self.wake();
        Ok(self.status())
    }

    /// Stops syncing and leaves the library. This device's music and edits stay as they are;
    /// the other devices carry on without it.
    pub(crate) fn leave_library(&self) -> Result<SyncStatus> {
        self.stop.store(true, Ordering::Release);
        let _one = self.round_lock.lock().unwrap_or_else(|e| e.into_inner());
        for name in ["key", "code", "library.json", "wifi.cbor"] {
            let _ = std::fs::remove_file(self.dir.join(name));
        }
        let _ = std::fs::remove_dir_all(self.store_dir());
        {
            let mut inner = self.inner();
            inner.library = None;
            inner.saved = None;
            self.save(&inner)?;
        }
        if let Ok(mut seen) = self.seen.lock() {
            seen.clear();
        }
        self.advertise();
        self.refresh_status();
        self.set_status(|s| {
            s.devices.clear();
            s.handoffs.clear();
            s.problems.clear();
            s.error = None;
            s.last_sync_ms = None;
        });
        Ok(self.status())
    }

    /// Other Cadenza devices on this network.
    pub(crate) fn nearby(&self) -> Vec<NearbyDevice> {
        let Some(d) = &self.discovery else {
            return Vec::new();
        };
        let me = self.device_id().unwrap_or_default();
        let mine = self.hello().library_id();
        d.found(&me)
            .into_iter()
            .map(|f| NearbyDevice {
                paired: mine.is_some() && f.library == mine,
                syncing: f.library.is_some(),
                address: f.addrs.first().map(ToString::to_string).unwrap_or_default(),
                id: f.device,
                name: f.name.chars().take(60).collect(),
                platform: f.platform.chars().take(20).collect(),
            })
            .collect()
    }

    pub(crate) fn sync_now(&self) {
        self.wake();
    }

    fn enabled(inner: &Inner) -> bool {
        inner.library.is_some() && inner.saved.is_some()
    }

    /// This device, as it signs requests to the others.
    fn me(&self) -> Option<Me> {
        let inner = self.inner();
        let device = inner.saved.as_ref()?.replica.device().to_string();
        let key = inner.library.as_ref()?.1.clone();
        Some(Me {
            device,
            port: self.port(),
            key,
        })
    }

    // ---- local changes ------------------------------------------------------------------

    /// Records local changes (applied here already) to be pushed.
    fn record(&self, changes: Vec<Change>) -> Vec<Hlc> {
        let mut out = Vec::new();
        {
            let mut inner = self.inner();
            if !Self::enabled(&inner) {
                return out;
            }
            let Some(saved) = inner.saved.as_mut() else {
                return out;
            };
            let wall = wall_ms();
            for change in changes {
                let register = register_of(&change);
                let track = change.track().clone();
                let op = saved.replica.record(change, wall);
                if let Some(r) = register {
                    saved.applied.insert((track, r), op.hlc);
                }
                out.push(op.hlc);
            }
            let _ = self.save(&inner);
        }
        self.refresh_status();
        self.wake();
        out
    }

    /// Tag fields written by an edit in Cadenza.
    pub(crate) fn tags_written(&self, track_id: i64, changes: Vec<(String, Vec<String>)>) {
        let Ok(hashes) = self.library.audio_hashes(&[track_id]) else {
            return;
        };
        let Some((_, hash)) = hashes.into_iter().next() else {
            return;
        };
        let track = TrackKey::new(hash);
        self.record(
            changes
                .into_iter()
                .map(|(field, values)| Change::SetField {
                    track: track.clone(),
                    field,
                    values,
                })
                .collect(),
        );
    }

    pub(crate) fn favourites(&self, ids: &[i64], on: bool) {
        let hashes: BTreeSet<String> = self
            .library
            .audio_hashes(ids)
            .unwrap_or_default()
            .into_iter()
            .map(|(_, h)| h)
            .collect();
        self.record(
            hashes
                .into_iter()
                .map(|h| Change::SetFavourite {
                    track: TrackKey::new(h),
                    on,
                })
                .collect(),
        );
    }

    pub(crate) fn trash(&self, hashes: Vec<String>, trashed: bool) {
        let unique: BTreeSet<String> = hashes.into_iter().collect();
        self.record(
            unique
                .into_iter()
                .map(|h| Change::Trash {
                    track: TrackKey::new(h),
                    trashed,
                })
                .collect(),
        );
    }

    /// A listen: recorded in the library, and as an op when sync is on.
    pub(crate) fn play(&self, track_id: i64, played_ms: i64, completed: bool) {
        let hash = self
            .library
            .audio_hashes(&[track_id])
            .ok()
            .and_then(|h| h.into_iter().next())
            .map(|(_, h)| h);
        let at = ms_to_s(wall_ms());
        let id = hash.and_then(|h| {
            self.record(vec![Change::Play {
                track: TrackKey::new(h),
                at,
                played_ms,
                completed,
            }])
            .first()
            .map(cadenza_sync::state::play_id)
        });
        let _ = self.library.record_play(track_id, played_ms, completed, id);
    }

    // ---- conflicts ----------------------------------------------------------------------

    pub(crate) fn conflicts(&self) -> Vec<SyncConflict> {
        let (conflicts, names) = {
            let inner = self.inner();
            let Some(saved) = inner.saved.as_ref() else {
                return Vec::new();
            };
            (saved.replica.state().conflicts(), self.device_names())
        };
        let hashes: Vec<String> = conflicts
            .iter()
            .map(|c| c.track.audio_hash.clone())
            .collect();
        let ids: HashMap<String, i64> = self
            .library
            .tracks_with_hashes(&hashes)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let rows: HashMap<i64, (String, String)> = self
            .library
            .tracks(&ids.values().copied().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.id, (t.title, t.artist)))
            .collect();
        let name = |h: &Hlc| {
            names
                .get(&h.device.to_string())
                .cloned()
                .unwrap_or_else(|| "another device".into())
        };
        conflicts
            .into_iter()
            .map(|c| {
                let row = ids.get(&c.track.audio_hash).and_then(|id| rows.get(id));
                SyncConflict {
                    audio_hash: c.track.audio_hash.clone(),
                    title: row.map(|r| r.0.clone()),
                    artist: row.map(|r| r.1.clone()),
                    field: c.field,
                    kept_device: name(&c.kept_at),
                    kept_at_ms: c.kept_at.ms,
                    kept: c.kept,
                    lost_device: name(&c.lost_at),
                    lost_at_ms: c.lost_at.ms,
                    lost: c.lost,
                }
            })
            .collect()
    }

    fn device_names(&self) -> HashMap<String, String> {
        self.status()
            .devices
            .into_iter()
            .map(|d| (d.id, d.name))
            .collect()
    }

    /// Settles a conflict: `values` replaces every version, on every device.
    pub(crate) fn resolve(&self, audio_hash: &str, field: &str, values: Vec<String>) -> Result<()> {
        let track = TrackKey::new(audio_hash);
        let flag = values.first().is_some_and(|v| v == "true");
        let change = match field {
            "favourite" => Change::SetFavourite { track, on: flag },
            "trash" => Change::Trash {
                track,
                trashed: flag,
            },
            _ => Change::SetField {
                track,
                field: field.to_owned(),
                values,
            },
        };
        {
            let mut inner = self.inner();
            let saved = inner
                .saved
                .as_mut()
                .ok_or_else(|| Error::Other("sync is off".into()))?;
            // Not marked applied: the next round writes the chosen value here too, if needed.
            saved.replica.resolve(change, wall_ms());
            self.save(&inner)?;
        }
        self.refresh_status();
        self.wake();
        Ok(())
    }

    // ---- a round ------------------------------------------------------------------------

    /// Runs a round now, on this thread.
    pub(crate) fn round(&self) {
        let _one = self.round_lock.lock().unwrap_or_else(|e| e.into_inner());
        let settings = {
            let inner = self.inner();
            if !Self::enabled(&inner) || inner.settings.paused {
                return;
            }
            inner.settings.clone()
        };
        self.stop.store(false, Ordering::Release);
        self.set_status(|s| {
            s.syncing = true;
            s.error = None;
        });
        let mut problems = Vec::new();
        let result = self.round_in(&settings, &mut problems);
        self.refresh_status();
        self.set_status(|s| {
            s.syncing = false;
            s.transfer = None;
            s.problems = problems;
            match result {
                Ok(()) => s.last_sync_ms = Some(wall_ms()),
                Err(e) => s.error = Some(e.to_string()),
            }
        });
    }

    fn round_in(&self, settings: &SyncSettings, problems: &mut Vec<String>) -> Result<()> {
        let Some(me) = self.me() else { return Ok(()) };
        let store = self.store();
        let sync_err = |e: cadenza_sync::Error| Error::Other(e.to_string());
        self.export_history()?;
        self.upload_art(&store, problems);

        // Copy what the paired devices in reach have, then push and pull through this device's
        // own store as if they all shared it. Listens are handled from the effects; everything
        // else from the merged state below.
        let peers = self.pull_peers(&store, &me, problems);
        let mut effects = Vec::new();
        let report = {
            let mut inner = self.inner();
            let Some(saved) = inner.saved.as_mut() else {
                return Ok(());
            };
            let info = (settings.device_name.as_str(), platform());
            let report = saved
                .replica
                .sync(&store, Some((info.0, &info.1)), wall_ms(), |e| {
                    effects.push(e)
                })
                .map_err(sync_err)?;
            self.save(&inner)?;
            report
        };
        problems.extend(report.unreadable);
        let online: BTreeSet<String> = peers.iter().map(|(id, _)| id.clone()).collect();
        self.read_devices(&store, &online);
        if let Err(e) = self.handoff(&store, settings) {
            problems.push(format!("Sharing what's playing: {e}"));
        }

        let mut changed = false;
        for effect in effects {
            if let Effect::Play {
                track,
                at,
                played_ms,
                completed,
                id,
            } = effect
                && let Some(&track_id) = self.library.tracks_with_hash(&track.audio_hash)?.first()
            {
                changed |= self
                    .library
                    .import_play(track_id, at, played_ms, completed, &id)?;
            }
        }
        changed |= self.catch_up_scanned()?;
        changed |= self.apply_pending(&store, problems)?;
        changed |= self.library.purge_expired()? > 0;
        if changed {
            (self.emit)(ApiEvent::LibraryChanged);
        }

        let mut news = report.pushed_ops > 0;
        if settings.share_audio {
            news |= self.share_audio(&store)?;
        }
        // The others copy this device's news now rather than within the minute.
        if news {
            for (_, p) in &peers {
                let _ = p.poke();
            }
        }
        if settings.download_audio {
            self.download_audio(&peers, problems)?;
        }
        Ok(())
    }

    /// Where paired devices might be: found on the network in this library, recently in touch,
    /// or typed in.
    fn targets(&self, me: &str) -> Vec<Target> {
        let mut out: Vec<Target> = Vec::new();
        let library = self.hello().library_id();
        if let Some(d) = &self.discovery {
            for f in d.found(me) {
                if library.is_some() && f.library == library {
                    out.push((Some(f.device), f.addrs));
                }
            }
        }
        if let Ok(mut seen) = self.seen.lock() {
            seen.retain(|_, (_, at)| at.elapsed() < SEEN_FOR);
            for (id, (addr, _)) in seen.iter() {
                match out
                    .iter_mut()
                    .find(|(d, _)| d.as_deref() == Some(id.as_str()))
                {
                    Some((_, addrs)) if !addrs.contains(addr) => addrs.push(*addr),
                    Some(_) => {}
                    None => out.push((Some(id.clone()), vec![*addr])),
                }
            }
        }
        for typed in self.inner().settings.addresses.clone() {
            if let Some(addr) = peer::parse_address(&typed)
                && !out.iter().any(|(_, a)| a.contains(&addr))
            {
                out.push((None, vec![addr]));
            }
        }
        out
    }

    /// Copies, from every paired device in reach, what it has and this device lacks. The
    /// devices reached, by id.
    fn pull_peers(
        &self,
        store: &FolderStore,
        me: &Me,
        problems: &mut Vec<String>,
    ) -> Vec<(String, Peer)> {
        let library = self.hello().library_id();
        let mut reached: Vec<(String, Peer)> = Vec::new();
        for (id, addrs) in self.targets(&me.device) {
            for addr in addrs {
                // A typed address: who's there, and is it in this library?
                let id = match &id {
                    Some(id) => id.clone(),
                    None => match peer::hello(addr) {
                        Ok(h) if h.library_id() == library && h.device != me.device => h.device,
                        _ => continue,
                    },
                };
                if reached.iter().any(|(r, _)| *r == id) {
                    break;
                }
                let p = Peer::new(addr, me.clone());
                match peer::pull(&p, store, &me.device, &id) {
                    Ok(_) => {
                        reached.push((id, p));
                        break;
                    }
                    Err(cadenza_sync::Error::Crypto(e)) => {
                        problems.push(format!("{addr}: {e}"));
                        break;
                    }
                    // Not there now (asleep, another network): it's tried again next round.
                    Err(_) => {}
                }
            }
        }
        reached
    }

    /// Publishes what this device is playing (if it changed) and reads what the others are.
    fn handoff(&self, store: &dyn Store, settings: &SyncSettings) -> Result<()> {
        let me = {
            let inner = self.inner();
            inner.saved.as_ref().map(|s| s.replica.device().to_string())
        };
        let Some(me) = me else { return Ok(()) };
        let state = self.player.state();
        let playing = state.status == cadenza_engine::Status::Playing;
        let mark = (
            state.queue_version,
            state.index,
            state.position_ms / 10_000,
            playing,
        );
        let changed = self
            .published
            .lock()
            .map(|p| *p != Some(mark))
            .unwrap_or(true);
        if changed && let (Some(index), Some(track)) = (state.index, state.track_id) {
            let queue = self.player.queue();
            // Up to HANDOFF_SONGS around the playing song.
            let start = index.saturating_sub(HANDOFF_SONGS / 2);
            let window: Vec<i64> = queue
                .iter()
                .skip(start)
                .take(HANDOFF_SONGS)
                .copied()
                .collect();
            let hashes: HashMap<i64, String> =
                self.library.audio_hashes(&window)?.into_iter().collect();
            let mut list = Vec::new();
            let mut at = 0;
            for (i, id) in window.iter().enumerate() {
                if let Some(h) = hashes.get(id) {
                    if start + i == index {
                        at = list.len();
                    }
                    list.push(h.clone());
                }
            }
            let row = self.library.tracks(&[track])?.into_iter().next();
            if !list.is_empty() {
                let file = HandoffFile {
                    device: me.clone(),
                    name: settings.device_name.clone(),
                    at_ms: wall_ms(),
                    queue: list,
                    index: at,
                    position_ms: state.position_ms,
                    title: row.as_ref().map(|r| r.title.clone()).unwrap_or_default(),
                    artist: row.map(|r| r.artist).unwrap_or_default(),
                };
                let json = serde_json::to_vec(&file).map_err(|e| Error::Other(e.to_string()))?;
                store
                    .put(&format!("handoff/{me}.json"), &json)
                    .map_err(other)?;
            }
            if let Ok(mut p) = self.published.lock() {
                *p = Some(mark);
            }
        }

        let cutoff = wall_ms().saturating_sub(HANDOFF_DAYS * 86_400_000);
        let mut others: Vec<HandoffFile> = store
            .list("handoff")
            .map_err(other)?
            .into_iter()
            .filter(|n| n.ends_with(".json") && *n != format!("{me}.json"))
            .filter_map(|n| store.get(&format!("handoff/{n}")).ok().flatten())
            .filter_map(|b| serde_json::from_slice::<HandoffFile>(&b).ok())
            .filter(|h| h.at_ms >= cutoff && h.index < h.queue.len())
            .collect();
        others.sort_by_key(|h| std::cmp::Reverse(h.at_ms));
        let mut shown = Vec::new();
        for h in &others {
            let here = self.library.tracks_with_hashes(&h.queue)?;
            let present: std::collections::HashSet<&String> = here.iter().map(|(k, _)| k).collect();
            shown.push(Handoff {
                device_id: h.device.clone(),
                device_name: h.name.chars().take(60).collect(),
                at_ms: h.at_ms,
                title: h.title.chars().take(200).collect(),
                artist: h.artist.chars().take(200).collect(),
                position_ms: h.position_ms,
                songs: h.queue.len(),
                here: h.queue.iter().filter(|q| present.contains(q)).count(),
            });
        }
        if let Ok(mut slot) = self.handoffs.lock() {
            *slot = others;
        }
        self.set_status(|s| s.handoffs = shown);
        Ok(())
    }

    /// Another device's listening as a queue of this device's songs.
    pub(crate) fn resume(&self, device_id: &str) -> Result<Resume> {
        let h = self
            .handoffs
            .lock()
            .ok()
            .and_then(|list| list.iter().find(|h| h.device == device_id).cloned())
            .ok_or_else(|| Error::Other("that device's listening is no longer on offer".into()))?;
        let mut first: HashMap<String, i64> = HashMap::new();
        for (hash, id) in self.library.tracks_with_hashes(&h.queue)? {
            first.entry(hash).or_insert(id);
        }
        let mut ids = Vec::new();
        let mut index = None;
        for (i, hash) in h.queue.iter().enumerate() {
            if let Some(&id) = first.get(hash) {
                if i >= h.index && index.is_none() {
                    index = Some(ids.len());
                }
                ids.push(id);
            }
        }
        let index = index.ok_or_else(|| {
            Error::Other("this device has none of the songs that were playing there".into())
        })?;
        // Only the song that was playing resumes part-way.
        let same = first
            .get(&h.queue[h.index])
            .is_some_and(|id| ids.get(index) == Some(id));
        Ok(Resume {
            ids,
            index,
            position_ms: if same { h.position_ms } else { 0 },
        })
    }

    /// Listens from before sync was on (or while it was off), and favourites the first time.
    fn export_history(&self) -> Result<()> {
        let plays = self.library.unsynced_plays()?;
        let favourites = {
            let inner = self.inner();
            match &inner.saved {
                Some(s) if !s.exported_favourites => self.library.favourite_hashes()?,
                _ => Vec::new(),
            }
        };
        let mut marks = Vec::new();
        {
            let mut inner = self.inner();
            let Some(saved) = inner.saved.as_mut() else {
                return Ok(());
            };
            let wall = wall_ms();
            for p in plays {
                let op = saved.replica.record(
                    Change::Play {
                        track: TrackKey::new(p.audio_hash),
                        at: p.at,
                        played_ms: p.played_ms,
                        completed: p.completed,
                    },
                    wall,
                );
                marks.push((p.id, cadenza_sync::state::play_id(&op.hlc)));
            }
            for h in favourites {
                let track = TrackKey::new(h);
                let op = saved.replica.record(
                    Change::SetFavourite {
                        track: track.clone(),
                        on: true,
                    },
                    wall,
                );
                saved.applied.insert((track, "favourite".into()), op.hlc);
            }
            saved.exported_favourites = true;
            self.save(&inner)?;
        }
        if !marks.is_empty() {
            self.library.mark_plays_synced(marks)?;
        }
        Ok(())
    }

    /// Uploads the images that queued picture changes refer to.
    fn upload_art(&self, store: &dyn Store, problems: &mut Vec<String>) {
        let hashes: BTreeSet<String> = {
            let inner = self.inner();
            let Some(saved) = inner.saved.as_ref() else {
                return;
            };
            saved
                .replica
                .outbox()
                .iter()
                .filter_map(|op| match &op.change {
                    Change::SetField { field, values, .. } if field == "pictures" => Some(
                        values
                            .iter()
                            .filter_map(|v| parse_picture(v))
                            .map(|p| p.hash),
                    ),
                    _ => None,
                })
                .flatten()
                .collect()
        };
        for hash in hashes {
            let Some(path) = art_path(&hash) else {
                continue;
            };
            if store.size(&path).ok().flatten().is_some() {
                continue;
            }
            let uploaded = self
                .library
                .artwork_original(&hash)
                .map_err(|e| e.to_string())
                .and_then(|local| {
                    store
                        .put_file(&path, &local, &mut |_| true)
                        .map_err(|e| e.to_string())
                });
            if let Err(e) = uploaded {
                problems.push(format!("Artwork {hash}: {e}"));
            }
        }
    }

    fn read_devices(&self, store: &dyn Store, online: &BTreeSet<String>) {
        let me = {
            let inner = self.inner();
            inner.saved.as_ref().map(|s| s.replica.device().to_string())
        };
        let mut devices: Vec<SyncDevice> = store
            .list("devices")
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| {
                let bytes = store.get(&format!("devices/{name}")).ok().flatten()?;
                let d: DeviceInfo = serde_json::from_slice(&bytes).ok()?;
                Some(SyncDevice {
                    this_device: Some(&d.id) == me.as_ref(),
                    online: online.contains(&d.id),
                    id: d.id,
                    name: d.name.chars().take(60).collect(),
                    platform: d.platform.chars().take(20).collect(),
                    last_push_ms: d.last_push_ms,
                })
            })
            .collect();
        devices.sort_by(|a, b| b.this_device.cmp(&a.this_device).then(a.name.cmp(&b.name)));
        self.set_status(|s| s.devices = devices);
    }

    /// Songs that turned up since last time (scanned, downloaded, restored) get their synced
    /// listens.
    fn catch_up_scanned(&self) -> Result<bool> {
        let since = {
            let inner = self.inner();
            inner.saved.as_ref().map_or(0, |s| s.scan_check)
        };
        let started = ms_to_s(wall_ms());
        let scanned = self.library.scanned_since(since)?;
        let plays: Vec<(i64, Vec<(String, cadenza_sync::state::PlayRecord)>)> = {
            let inner = self.inner();
            let Some(saved) = inner.saved.as_ref() else {
                return Ok(false);
            };
            scanned
                .into_iter()
                .map(|(id, h)| (id, saved.replica.state().plays_of(&TrackKey::new(h))))
                .filter(|(_, p)| !p.is_empty())
                .collect()
        };
        let mut changed = false;
        for (track_id, list) in plays {
            for (sync_id, p) in list {
                changed |=
                    self.library
                        .import_play(track_id, p.at, p.played_ms, p.completed, &sync_id)?;
            }
        }
        let mut inner = self.inner();
        if let Some(saved) = inner.saved.as_mut() {
            saved.scan_check = started;
        }
        self.save(&inner)?;
        Ok(changed)
    }

    /// Applies every value whose current version hasn't been applied here yet.
    fn apply_pending(&self, store: &dyn Store, problems: &mut Vec<String>) -> Result<bool> {
        let pending: Vec<Pending> = {
            let inner = self.inner();
            let Some(saved) = inner.saved.as_ref() else {
                return Ok(false);
            };
            saved
                .replica
                .state()
                .winners()
                .into_iter()
                .filter(|(t, r, h, _)| {
                    saved.applied.get(&((*t).clone(), (*r).to_owned())) != Some(h)
                })
                .map(|(t, r, h, v)| (t.clone(), r.to_owned(), h, v))
                .collect()
        };
        if pending.is_empty() {
            return Ok(false);
        }
        let hashes: Vec<String> = pending
            .iter()
            .map(|p| p.0.audio_hash.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut present: HashMap<String, Vec<i64>> = HashMap::new();
        for (h, id) in self.library.tracks_with_hashes(&hashes)? {
            present.entry(h).or_default().push(id);
        }
        let mut done: Vec<(TrackKey, String, Hlc)> = Vec::new();
        let mut changed = false;
        let mut tag_work: BTreeMap<i64, Vec<Pending>> = BTreeMap::new();
        let mut rescan = false;
        for (track, register, hlc, values) in pending {
            let ids = present.get(&track.audio_hash).cloned().unwrap_or_default();
            let flag = values.first().is_some_and(|v| v == "true");
            match register.as_str() {
                "favourite" => {
                    if ids.is_empty() {
                        continue;
                    }
                    self.library.set_favourite(&ids, flag)?;
                    changed = true;
                }
                "trash" => {
                    if flag && !ids.is_empty() {
                        if let Err(e) = self.library.trash(&ids) {
                            problems.push(format!("Moving a song to the trash: {e}"));
                        }
                        changed = true;
                    } else if !flag {
                        for item in self.library.trashed_with_hash(&track.audio_hash)? {
                            match self.library.restore(item) {
                                Ok(()) => rescan = true,
                                Err(e) => problems.push(format!("Restoring a song: {e}")),
                            }
                        }
                    }
                }
                _ => {
                    // Tag fields wait for a song that's here, and for the tag writer.
                    if ids.is_empty() || self.editor.busy() {
                        continue;
                    }
                    for id in ids {
                        tag_work.entry(id).or_default().push((
                            track.clone(),
                            register.clone(),
                            hlc,
                            values.clone(),
                        ));
                    }
                    continue;
                }
            }
            done.push((track, register, hlc));
        }
        if !tag_work.is_empty() {
            let ids: Vec<i64> = tag_work.keys().copied().collect();
            let tags: HashMap<i64, crate::TrackTags> = self
                .editor
                .tags(&ids)
                .unwrap_or_default()
                .into_iter()
                .map(|t| (t.track_id, t))
                .collect();
            let mut edits = Vec::new();
            for (id, work) in tag_work {
                let current = tags.get(&id);
                let mut edit = TrackEdit {
                    track_id: id,
                    fields: BTreeMap::new(),
                    pictures: None,
                };
                for (track, field, hlc, values) in work {
                    done.push((track, field.clone(), hlc));
                    let Some(current) = current else {
                        problems.push(format!("A song's tags couldn't be read (id {id})"));
                        continue;
                    };
                    if let Some(why) = &current.read_only {
                        problems.push(format!("{}: {why}", current.path));
                        continue;
                    }
                    if field == "pictures" {
                        let now: Vec<String> = current
                            .pictures
                            .iter()
                            .map(|p| {
                                format!(
                                    "{}:{}:{}",
                                    p.picture_type,
                                    p.hash.as_deref().unwrap_or("?"),
                                    p.description
                                )
                            })
                            .collect();
                        if now != values {
                            let refs: Vec<_> =
                                values.iter().filter_map(|v| parse_picture(v)).collect();
                            if refs.iter().all(|r| self.fetch_art(store, &r.hash)) {
                                edit.pictures = Some(PictureEdit::Replace { pictures: refs });
                            } else {
                                problems.push(format!(
                                    "{}: artwork from another device isn't in the sync folder yet",
                                    current.path
                                ));
                            }
                        }
                    } else if current.fields.get(&field).cloned().unwrap_or_default() != values {
                        edit.fields.insert(field, values);
                    }
                }
                if !edit.fields.is_empty() || edit.pictures.is_some() {
                    edits.push(edit);
                }
            }
            if !edits.is_empty() {
                let n = edits.len();
                let description = if n == 1 {
                    "Changes from another device (1 song)".to_owned()
                } else {
                    format!("Changes from other devices ({n} songs)")
                };
                if let Err(e) = self.editor.apply_synced(
                    edits,
                    &description,
                    self.backups.load(Ordering::Acquire),
                ) {
                    // Busy after all: leave these for the next round.
                    problems.push(format!("Tag changes from other devices wait: {e}"));
                    done.retain(|(_, f, _)| f == "favourite" || f == "trash");
                }
            }
        }
        if rescan {
            self.scanner.request();
        }
        let mut inner = self.inner();
        if let Some(saved) = inner.saved.as_mut() {
            for (track, register, hlc) in done {
                saved.applied.insert((track, register), hlc);
            }
        }
        self.save(&inner)?;
        Ok(changed)
    }

    /// Makes sure an image is in the artwork cache, from the folder if need be.
    fn fetch_art(&self, store: &dyn Store, hash: &str) -> bool {
        if self.library.artwork_original(hash).is_ok() {
            return true;
        }
        let Some(path) = art_path(hash) else {
            return false;
        };
        match store.get(&path) {
            Ok(Some(bytes)) => {
                matches!(self.library.store_artwork(&bytes), Ok(Some(s)) if s.hash == hash)
            }
            _ => false,
        }
    }

    fn progress(
        &self,
        direction: &str,
        name: &str,
        total: u64,
        remaining: usize,
    ) -> impl FnMut(u64) -> bool + '_ {
        let mut last = Instant::now() - Duration::from_secs(1);
        let (direction, name) = (direction.to_owned(), name.to_owned());
        move |done| {
            if last.elapsed() > Duration::from_millis(250) || done == total {
                last = Instant::now();
                let t = Transfer {
                    direction: direction.clone(),
                    name: name.clone(),
                    done,
                    total,
                    remaining,
                };
                self.set_status(|s| s.transfer = Some(t));
            }
            !self.stop.load(Ordering::Acquire)
        }
    }

    /// Offers this device's songs that no device has offered yet: the others can then copy them
    /// from here. True if there were any.
    fn share_audio(&self, store: &FolderStore) -> Result<bool> {
        let todo: Vec<cadenza_library::TrackFile> = {
            let inner = self.inner();
            let Some(saved) = inner.saved.as_ref() else {
                return Ok(false);
            };
            let state = saved.replica.state();
            self.library
                .track_files()?
                .into_iter()
                .filter(|f| state.shared(&TrackKey::new(f.audio_hash.clone())).is_none())
                .collect()
        };
        if todo.is_empty() {
            return Ok(false);
        }
        self.record(
            todo.into_iter()
                .map(|f| Change::Shared {
                    track: TrackKey::new(f.audio_hash),
                    rel_path: f.rel_path.replace('\\', "/"),
                    size: f.size,
                })
                .collect(),
        );
        self.push_only(store)?;
        Ok(true)
    }

    fn push_only(&self, store: &FolderStore) -> Result<()> {
        let mut inner = self.inner();
        let name = inner.settings.device_name.clone();
        if let Some(saved) = inner.saved.as_mut() {
            saved
                .replica
                .push(store, Some((&name, &platform())), wall_ms())
                .map_err(|e| Error::Other(e.to_string()))?;
        }
        self.save(&inner)
    }

    /// Copies songs other devices offer that this device doesn't have into its first music
    /// folder, from whichever device in reach has each.
    fn download_audio(&self, peers: &[(String, Peer)], problems: &mut Vec<String>) -> Result<()> {
        let Some(root) = self.library.roots()?.into_iter().next() else {
            problems.push("Add a music folder to copy songs from other devices into.".into());
            return Ok(());
        };
        let shared: Vec<(String, String, u64, bool)> = {
            let inner = self.inner();
            let Some(saved) = inner.saved.as_ref() else {
                return Ok(());
            };
            let state = saved.replica.state();
            state
                .all_shared()
                .map(|(k, a)| {
                    (
                        k.audio_hash.clone(),
                        a.rel_path.clone(),
                        a.size,
                        state.trashed(k) == Some(true),
                    )
                })
                .collect()
        };
        let mut todo = Vec::new();
        for (hash, rel, size, trashed) in shared {
            if !trashed && !self.library.knows_hash(&hash)? {
                todo.push((hash, rel, size));
            }
        }
        let total = todo.len();
        let mut got = 0;
        let mut away = 0;
        for (i, (hash, rel, size)) in todo.into_iter().enumerate() {
            let Some(rel_path) = safe_relative(&rel) else {
                problems.push(format!("A shared song has an unsafe path: {rel}"));
                continue;
            };
            let ext = rel_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let Some(blob) = blob_path(&hash, &ext) else {
                continue;
            };
            let mut dest = Path::new(&root.path).join(&rel_path);
            if dest.exists() {
                // Something else is already there: keep both.
                let stem = dest
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                dest = dest.with_file_name(format!("{stem} (synced).{ext}"));
                if dest.exists() {
                    continue;
                }
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::Other(e.to_string()))?;
            }
            let name = dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut done = false;
            for (_, p) in peers {
                let mut go = self.progress("download", &name, size, total - i - 1);
                match p.blob_to(&blob, &dest, &mut go) {
                    Ok(true) => {
                        got += 1;
                        done = true;
                        break;
                    }
                    // Stopped (paused, or settings changed).
                    Ok(false) => return Ok(()),
                    // That device doesn't have it (or doesn't share): try the next.
                    Err(_) => {}
                }
            }
            if !done {
                away += 1;
            }
            if got > 0 && got % 10 == 0 {
                self.scanner.request();
            }
        }
        if got > 0 {
            self.scanner.request();
        }
        if away > 0 {
            problems.push(format!(
                "{away} {} on devices that aren't sharing on this network right now.",
                if away == 1 { "song is" } else { "songs are" }
            ));
        }
        Ok(())
    }
}

/// This device's address on the local network: the one it would use to reach beyond it. No
/// packet is sent.
fn local_addresses() -> Vec<std::net::IpAddr> {
    let probe = |bind: &str, to: &str| {
        let s = std::net::UdpSocket::bind(bind).ok()?;
        s.connect(to).ok()?;
        let ip = s.local_addr().ok()?.ip();
        (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
    };
    probe("0.0.0.0:0", "192.0.2.1:9")
        .into_iter()
        .chain(probe("[::]:0", "[2001:db8::1]:9"))
        .collect()
}

/// A value not yet applied here: song, register, its version, the value.
type Pending = (TrackKey, String, Hlc, Vec<String>);

/// The register an op changes, for the applied map; none for listens and shares.
fn register_of(change: &Change) -> Option<String> {
    match change {
        Change::SetField { field, .. } => Some(field.clone()),
        Change::SetFavourite { .. } => Some("favourite".into()),
        Change::Trash { .. } => Some("trash".into()),
        Change::Play { .. } | Change::Shared { .. } => None,
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn run(me: std::sync::Weak<Syncer>, wake: Receiver<()>) {
    loop {
        match wake.recv_timeout(INTERVAL) {
            Ok(()) => {
                // Let a burst of changes settle into one round.
                thread::sleep(SETTLE);
                while wake.try_recv().is_ok() {}
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let Some(syncer) = me.upgrade() else { return };
        syncer.round();
    }
}
