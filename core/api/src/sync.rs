//! Sync in the app (D55, D56): turns this device's tag edits, favourites, listens and trash into
//! ops, and applies other devices' ops to this device's own files and catalogue.
//!
//! Everything lives in `<data>/sync/`: `config.json` (the folder, this device's name, sharing
//! options) and `replica.cbor` (the merged state, what has been applied here, the outbox).
//!
//! A background thread syncs a minute after the last sync, a few seconds after a local change, or
//! when asked. Each round: push this device's ops, pull the others', then apply whatever hasn't
//! been applied here yet. Applying is driven by the merged state, not by the ops as they arrive,
//! so a change for a song this device doesn't have yet applies when the song turns up, and a
//! change that couldn't be applied (the tag writer was busy) is tried again next round.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cadenza_library::Library;
use cadenza_sync::hlc::wall_ms;
use cadenza_sync::replica::DeviceInfo;
use cadenza_sync::{
    Change, DeviceId, Effect, EncryptedStore, FolderStore, Hlc, Key, LibraryInfo, Replica, Store,
    TrackKey,
};
use serde::{Deserialize, Serialize};

use crate::edit::{Editor, PictureEdit, TrackEdit, parse_picture};
use crate::{ApiEvent, Emit, Error, Result, Scanner};

/// How often to sync when nothing asks sooner.
const INTERVAL: Duration = Duration::from_secs(60);
/// How long to wait after a local change, so a burst of changes goes in one segment.
const SETTLE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncSettings {
    /// The shared folder; sync is off without one.
    pub folder: Option<String>,
    /// How other devices see this one.
    pub device_name: String,
    /// Copy this device's songs into the folder, for devices that don't have them.
    pub share_audio: bool,
    /// Copy songs other devices shared into this device's first music folder.
    pub download_audio: bool,
    /// Stop syncing until turned back on.
    pub paused: bool,
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
    pub syncing: bool,
    pub last_sync_ms: Option<u64>,
    pub error: Option<String>,
    /// Changes made here and not yet in the folder.
    pub pending: usize,
    pub devices: Vec<SyncDevice>,
    pub conflicts: usize,
    pub transfer: Option<Transfer>,
    /// Things that couldn't be synced and why (one line each, most recent round).
    pub problems: Vec<String>,
    /// The folder's contents are encrypted with a passphrase.
    pub encrypted: bool,
}

/// What a folder chosen for sync already holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFolderInfo {
    pub has_library: bool,
    pub encrypted: bool,
    pub devices: usize,
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

/// What `replica.cbor` holds.
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

impl Syncer {
    pub(crate) fn new(
        library: Arc<Library>,
        editor: Arc<Editor>,
        scanner: Arc<Scanner>,
        emit: Emit,
        data_dir: &Path,
    ) -> Arc<Self> {
        let dir = data_dir.join("sync");
        let mut settings: SyncSettings = std::fs::read(dir.join("config.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if settings.device_name.trim().is_empty() {
            settings.device_name = host_name();
        }
        let mut saved = std::fs::read(dir.join("replica.cbor"))
            .ok()
            .and_then(|b| ciborium::from_reader::<Saved, _>(b.as_slice()).ok());
        // A folder but no state (the state file was lost): start again as a new device, which
        // reads everything from the folder.
        if settings.folder.is_some() && saved.is_none() {
            saved = Some(Saved::new());
        }
        let me = Arc::new(Self {
            library,
            editor,
            scanner,
            emit,
            dir,
            inner: Mutex::new(Inner { settings, saved }),
            status: Mutex::new(SyncStatus::default()),
            wake: Mutex::new(None),
            stop: AtomicBool::new(false),
            round_lock: Mutex::new(()),
            backups: AtomicBool::new(false),
        });
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

    fn save(&self, inner: &Inner) -> Result<()> {
        let io = |e: std::io::Error| Error::Other(format!("saving sync state: {e}"));
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let json =
            serde_json::to_vec_pretty(&inner.settings).map_err(|e| Error::Other(e.to_string()))?;
        write_atomic(&self.dir.join("config.json"), &json).map_err(io)?;
        if let Some(saved) = &inner.saved {
            let mut bytes = Vec::new();
            ciborium::into_writer(saved, &mut bytes).map_err(|e| Error::Other(e.to_string()))?;
            write_atomic(&self.dir.join("replica.cbor"), &bytes).map_err(io)?;
        }
        Ok(())
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
        (self.emit)(ApiEvent::Sync { status });
    }

    /// Copies settings, device and counts into the status.
    fn refresh_status(&self) {
        let encrypted = self.load_key().is_some();
        let (settings, device, pending, conflicts) = {
            let inner = self.inner();
            let saved = inner.saved.as_ref();
            (
                inner.settings.clone(),
                saved.map(|s| s.replica.device().to_string()),
                saved.map_or(0, |s| s.replica.pending()),
                saved.map_or(0, |s| s.replica.state().conflicts().len()),
            )
        };
        self.set_status(|s| {
            s.settings = settings;
            s.device_id = device;
            s.pending = pending;
            s.conflicts = conflicts;
            s.encrypted = encrypted;
        });
    }

    pub(crate) fn set_settings(
        &self,
        mut settings: SyncSettings,
        passphrase: Option<&str>,
    ) -> Result<SyncStatus> {
        if settings.device_name.trim().is_empty() {
            settings.device_name = host_name();
        }
        settings.device_name = settings.device_name.trim().chars().take(60).collect();
        let changed_folder = settings.folder != self.inner().settings.folder;
        if let Some(folder) = &settings.folder {
            if !Path::new(folder).is_dir() {
                return Err(Error::Other(format!("{folder} isn't a folder")));
            }
            let roots = self.library.roots()?;
            if roots.iter().any(|r| {
                Path::new(folder).starts_with(&r.path) || Path::new(&r.path).starts_with(folder)
            }) {
                return Err(Error::Other(
                    "the sync folder must be outside your music folders, or Cadenza would add the songs in it twice"
                        .into(),
                ));
            }
            // Settle encryption before anything is written to the folder.
            let plain = FolderStore::new(folder);
            let info = LibraryInfo::read(&plain).map_err(other)?;
            match (&info, passphrase) {
                (
                    Some(LibraryInfo {
                        encryption: Some(e),
                        ..
                    }),
                    Some(p),
                ) => {
                    let key = Key::unlock(p, e).map_err(other)?;
                    self.save_key(Some(&key))?;
                }
                (
                    Some(LibraryInfo {
                        encryption: Some(_),
                        ..
                    }),
                    None,
                ) if changed_folder => {
                    return Err(Error::Other(
                        "this sync folder is encrypted: enter its passphrase".into(),
                    ));
                }
                (
                    Some(LibraryInfo {
                        encryption: None, ..
                    }),
                    Some(_),
                ) => {
                    return Err(Error::Other(
                        "this sync folder already holds unencrypted sync data; to encrypt, start with an empty folder"
                            .into(),
                    ));
                }
                (None, Some(p)) => {
                    let (key, encryption) = Key::create(p).map_err(other)?;
                    LibraryInfo {
                        format: cadenza_sync::crypt::FORMAT,
                        encryption: Some(encryption),
                    }
                    .write(&plain)
                    .map_err(other)?;
                    self.save_key(Some(&key))?;
                }
                (_, None) if changed_folder => self.save_key(None)?,
                _ => {}
            }
        }
        {
            let mut inner = self.inner();
            // A different folder starts afresh: this device's log there begins at the start.
            if settings.folder.is_some() && (inner.saved.is_none() || changed_folder) {
                inner.saved = Some(Saved::new());
            }
            inner.settings = settings;
            self.save(&inner)?;
        }
        // A transfer under the old settings stops; the next round starts under the new ones.
        self.stop.store(true, Ordering::Release);
        self.refresh_status();
        self.wake();
        Ok(self.status())
    }

    /// What's in a folder chosen for sync: nothing yet, a library, and whether it's encrypted.
    pub(crate) fn folder_info(&self, folder: &str) -> Result<SyncFolderInfo> {
        let plain = FolderStore::new(folder);
        let info = LibraryInfo::read(&plain).map_err(other)?;
        Ok(SyncFolderInfo {
            has_library: info.is_some() || !plain.list("log").map_err(other)?.is_empty(),
            encrypted: info.is_some_and(|i| i.encryption.is_some()),
            devices: plain.list("devices").map_err(other)?.len(),
        })
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join("key")
    }

    /// Keeps the library key (or forgets it), readable only by this user.
    fn save_key(&self, key: Option<&Key>) -> Result<()> {
        let path = self.key_path();
        let Some(key) = key else {
            let _ = std::fs::remove_file(&path);
            return Ok(());
        };
        let io = |e: std::io::Error| Error::Other(format!("saving the sync key: {e}"));
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let tmp = path.with_extension("tmp");
        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
            let mut f = opts.open(&tmp).map_err(io)?;
            std::io::Write::write_all(&mut f, &key.to_bytes()).map_err(io)?;
        }
        std::fs::rename(&tmp, &path).map_err(io)
    }

    fn load_key(&self) -> Option<Key> {
        std::fs::read(self.key_path())
            .ok()
            .and_then(|b| Key::from_bytes(&b))
    }

    /// The folder as a store: encrypted when the library is.
    fn open_store(&self, folder: &str) -> Result<Box<dyn Store>> {
        let plain = FolderStore::new(folder);
        let info = LibraryInfo::read(&plain).map_err(other)?;
        let key = self.load_key();
        match (info, key) {
            (None, None) => {
                LibraryInfo {
                    format: cadenza_sync::crypt::FORMAT,
                    encryption: None,
                }
                .write(&plain)
                .map_err(other)?;
                Ok(Box::new(plain))
            }
            (Some(LibraryInfo { encryption: None, .. }), None) => Ok(Box::new(plain)),
            (Some(LibraryInfo { encryption: Some(_), .. }), Some(key)) => Ok(Box::new(
                EncryptedStore::new(plain, key, self.dir.join("tmp")),
            )),
            (Some(LibraryInfo { encryption: Some(_), .. }), None) => Err(Error::Other(
                "the sync folder is encrypted: enter its passphrase on the Sync page".into(),
            )),
            (_, Some(_)) => Err(Error::Other(
                "the sync folder isn't encrypted the way this device expects (was it replaced?); choose it again"
                    .into(),
            )),
        }
    }

    pub(crate) fn sync_now(&self) {
        self.wake();
    }

    fn enabled(inner: &Inner) -> bool {
        inner.settings.folder.is_some() && inner.saved.is_some()
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
        let (folder, settings) = {
            let inner = self.inner();
            if !Self::enabled(&inner) || inner.settings.paused {
                return;
            }
            (inner.settings.folder.clone(), inner.settings.clone())
        };
        let Some(folder) = folder else { return };
        self.stop.store(false, Ordering::Release);
        self.set_status(|s| {
            s.syncing = true;
            s.error = None;
        });
        let mut problems = Vec::new();
        let result = self.round_in(&folder, &settings, &mut problems);
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

    fn round_in(
        &self,
        folder: &str,
        settings: &SyncSettings,
        problems: &mut Vec<String>,
    ) -> Result<()> {
        if !Path::new(folder).is_dir() {
            return Err(Error::Other(format!(
                "the sync folder {folder} isn't there (is its drive connected?)"
            )));
        }
        let boxed = self.open_store(folder)?;
        let store: &dyn Store = boxed.as_ref();
        let sync_err = |e: cadenza_sync::Error| Error::Other(e.to_string());
        self.export_history()?;
        self.upload_art(store, problems);

        // Push and pull. Listens and shared songs are handled from the effects; everything else
        // from the merged state below.
        let mut effects = Vec::new();
        let report = {
            let mut inner = self.inner();
            let Some(saved) = inner.saved.as_mut() else {
                return Ok(());
            };
            let info = (settings.device_name.as_str(), platform());
            let report = saved
                .replica
                .sync(store, Some((info.0, &info.1)), wall_ms(), |e| {
                    effects.push(e)
                })
                .map_err(sync_err)?;
            self.save(&inner)?;
            report
        };
        problems.extend(report.unreadable);
        self.read_devices(store);

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
        changed |= self.apply_pending(store, problems)?;
        changed |= self.library.purge_expired()? > 0;
        if changed {
            (self.emit)(ApiEvent::LibraryChanged);
        }

        if settings.share_audio {
            self.share_audio(store, problems)?;
        }
        if settings.download_audio {
            self.download_audio(store, problems)?;
        }
        Ok(())
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

    fn read_devices(&self, store: &dyn Store) {
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

    /// Copies songs into the folder that no device has shared yet.
    fn share_audio(&self, store: &dyn Store, problems: &mut Vec<String>) -> Result<()> {
        let todo: Vec<cadenza_library::TrackFile> = {
            let inner = self.inner();
            let Some(saved) = inner.saved.as_ref() else {
                return Ok(());
            };
            let state = saved.replica.state();
            self.library
                .track_files()?
                .into_iter()
                .filter(|f| state.shared(&TrackKey::new(f.audio_hash.clone())).is_none())
                .collect()
        };
        let total = todo.len();
        for (i, file) in todo.into_iter().enumerate() {
            let (hash, rel, size, path) = (file.audio_hash, file.rel_path, file.size, file.path);
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let Some(blob) = blob_path(&hash, &ext) else {
                continue;
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let there = store.size(&blob).ok().flatten() == Some(size);
            if !there {
                let mut go = self.progress("upload", &name, size, total - i - 1);
                match store.put_file(&blob, &path, &mut go) {
                    Ok(true) => {}
                    Ok(false) => return Ok(()),
                    Err(e) => {
                        problems.push(format!("Sharing {}: {e}", path.display()));
                        continue;
                    }
                }
            }
            self.record(vec![Change::Shared {
                track: TrackKey::new(hash),
                rel_path: rel.replace('\\', "/"),
                size,
            }]);
            // Push now and then, so other devices can start on a long upload.
            if i % 20 == 19 {
                self.push_only(store)?;
            }
        }
        self.push_only(store)
    }

    fn push_only(&self, store: &dyn Store) -> Result<()> {
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

    /// Copies songs other devices shared that this device doesn't have into its first music
    /// folder.
    fn download_audio(&self, store: &dyn Store, problems: &mut Vec<String>) -> Result<()> {
        let Some(root) = self.library.roots()?.into_iter().next() else {
            problems.push("Add a music folder to download songs from other devices into.".into());
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
            let mut go = self.progress("download", &name, size, total - i - 1);
            match store.get_file(&blob, &dest, &mut go) {
                Ok(true) => got += 1,
                Ok(false) => break,
                Err(e) => problems.push(format!("Downloading {rel}: {e}")),
            }
            if got > 0 && got % 10 == 0 {
                self.scanner.request();
            }
        }
        if got > 0 {
            self.scanner.request();
        }
        Ok(())
    }
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
