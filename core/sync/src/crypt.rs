//! Optional encryption of everything in the store (D56), so whoever holds the folder (a cloud
//! drive, a shared disk) sees only opaque data.
//!
//! - The key comes from a passphrase with Argon2id. Its salt and cost, and a check value that
//!   tells a wrong passphrase from a right one, are in `library.json`, which isn't encrypted.
//! - Small objects (log segments, device files, artwork) are sealed whole with
//!   XChaCha20-Poly1305: `CDZE`, a random 24-byte nonce, then the ciphertext.
//! - Songs are encrypted in 1 MiB chunks (the AEAD "STREAM" construction) so they never have to
//!   be in memory at once: `CDZF`, a random 19-byte nonce prefix, then the chunks.
//! - Every object is bound to its path (associated data), so objects can't be swapped around.
//! - Blob and artwork names are replaced with keyed hashes, so the folder doesn't reveal which
//!   songs are in it.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::stream::{DecryptorBE32, EncryptorBE32};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload, rand_core::RngCore};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::store::{Store, part_path};
use crate::{Error, Result};

/// Describes the library in the store: format, and encryption if any. Never encrypted.
pub const INFO_PATH: &str = "library.json";
pub const FORMAT: u32 = 1;

const SEALED: &[u8; 4] = b"CDZE";
const STREAMED: &[u8; 4] = b"CDZF";
const CHUNK: usize = 1 << 20;
const TAG: usize = 16;
const STREAM_NONCE: usize = 19;
const CHECK: &[u8] = b"cadenza library key check";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryInfo {
    pub format: u32,
    #[serde(default)]
    pub encryption: Option<Encryption>,
}

impl LibraryInfo {
    pub fn read(store: &dyn Store) -> Result<Option<Self>> {
        match store.get(INFO_PATH)? {
            None => Ok(None),
            Some(b) => serde_json::from_slice(&b).map(Some).map_err(|e| {
                Error::Crypto(format!("the sync folder's {INFO_PATH} is damaged: {e}"))
            }),
        }
    }

    pub fn write(&self, store: &dyn Store) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| Error::Encode(e.to_string()))?;
        store.put(INFO_PATH, &json)
    }
}

/// How the key is made from the passphrase, and a check value sealed with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encryption {
    /// Always "argon2id".
    pub kdf: String,
    /// Hex.
    pub salt: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    /// `CHECK` sealed with the key, hex.
    pub check: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn crypto_err(what: &str) -> Error {
    Error::Crypto(format!(
        "couldn't decrypt {what} (wrong passphrase, or the file is damaged)"
    ))
}

/// The library key: one half encrypts, the other names blobs.
#[derive(Clone)]
pub struct Key {
    data: [u8; 32],
    names: [u8; 32],
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}

impl Key {
    fn derive_raw(passphrase: &str, e: &Encryption) -> Result<Self> {
        let salt = unhex(&e.salt).ok_or_else(|| Error::Crypto("bad salt".into()))?;
        let params = Params::new(e.memory_kib, e.iterations, e.parallelism, Some(64))
            .map_err(|err| Error::Crypto(err.to_string()))?;
        let mut out = [0u8; 64];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(passphrase.as_bytes(), &salt, &mut out)
            .map_err(|err| Error::Crypto(err.to_string()))?;
        let mut data = [0u8; 32];
        let mut names = [0u8; 32];
        data.copy_from_slice(&out[..32]);
        names.copy_from_slice(&out[32..]);
        Ok(Self { data, names })
    }

    /// The key for an encrypted library, if the passphrase is right.
    pub fn unlock(passphrase: &str, e: &Encryption) -> Result<Self> {
        if e.kdf != "argon2id" {
            return Err(Error::Crypto(format!("unknown key derivation {}", e.kdf)));
        }
        let key = Self::derive_raw(passphrase, e)?;
        let check = unhex(&e.check).ok_or_else(|| Error::Crypto("bad check".into()))?;
        match key.open(&check, "check") {
            Ok(plain) if plain == CHECK => Ok(key),
            _ => Err(Error::Crypto(
                "that passphrase isn't the one this library was encrypted with".into(),
            )),
        }
    }

    /// A new key for a new encrypted library, with what goes in `library.json`.
    pub fn create(passphrase: &str) -> Result<(Self, Encryption)> {
        Self::create_with(passphrase, 64 * 1024, 3)
    }

    /// As `create`, with a cheaper key derivation (tests).
    pub fn create_with(
        passphrase: &str,
        memory_kib: u32,
        iterations: u32,
    ) -> Result<(Self, Encryption)> {
        if passphrase.chars().count() < 8 {
            return Err(Error::Crypto(
                "use a passphrase of at least 8 characters".into(),
            ));
        }
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        let mut e = Encryption {
            kdf: "argon2id".into(),
            salt: hex(&salt),
            memory_kib,
            iterations,
            parallelism: 1,
            check: String::new(),
        };
        let key = Self::derive_raw(passphrase, &e)?;
        e.check = hex(&key.seal(CHECK, "check")?);
        Ok((key, e))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        [self.data.as_slice(), self.names.as_slice()].concat()
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        (b.len() == 64).then(|| {
            let mut data = [0u8; 32];
            let mut names = [0u8; 32];
            data.copy_from_slice(&b[..32]);
            names.copy_from_slice(&b[32..]);
            Self { data, names }
        })
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.data).into())
    }

    fn seal(&self, plain: &[u8], path: &str) -> Result<Vec<u8>> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ct = self
            .cipher()
            .encrypt(
                &nonce,
                Payload {
                    msg: plain,
                    aad: path.as_bytes(),
                },
            )
            .map_err(|_| Error::Encode("encryption failed".into()))?;
        Ok([SEALED.as_slice(), nonce.as_slice(), &ct].concat())
    }

    fn open(&self, sealed: &[u8], path: &str) -> Result<Vec<u8>> {
        let body = sealed
            .strip_prefix(SEALED.as_slice())
            .ok_or_else(|| crypto_err(path))?;
        if body.len() < 24 + TAG {
            return Err(crypto_err(path));
        }
        let (nonce, ct) = body.split_at(24);
        self.cipher()
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ct,
                    aad: path.as_bytes(),
                },
            )
            .map_err(|_| crypto_err(path))
    }

    /// A blob's or image's name in the store: a keyed hash of its real path.
    fn name(&self, path: &str) -> String {
        let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(&self.names) else {
            return hex(path.as_bytes());
        };
        mac.update(path.as_bytes());
        hex(&mac.finalize().into_bytes()[..16])
    }
}

/// Plaintext size of a streamed object from its stored size.
fn plain_size(stored: u64) -> Option<u64> {
    let body = stored.checked_sub((STREAMED.len() + STREAM_NONCE) as u64)?;
    let chunk = (CHUNK + TAG) as u64;
    let rem = body % chunk;
    (rem >= TAG as u64).then(|| (body / chunk) * CHUNK as u64 + rem - TAG as u64)
}

/// Encrypts `from` into `to` in chunks; `go` hears plaintext bytes done and can stop it.
fn encrypt_file(
    key: &Key,
    path: &str,
    from: &Path,
    to: &Path,
    go: &mut dyn FnMut(u64) -> bool,
) -> Result<bool> {
    let mut src = fs::File::open(from)?;
    let mut out = fs::File::create(to)?;
    let mut prefix = [0u8; STREAM_NONCE];
    OsRng.fill_bytes(&mut prefix);
    out.write_all(STREAMED)?;
    out.write_all(&prefix)?;
    let mut enc = EncryptorBE32::from_aead(key.cipher(), (&prefix).into());
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    loop {
        if !go(done) {
            return Ok(false);
        }
        let n = read_full(&mut src, &mut buf)?;
        let payload = Payload {
            msg: &buf[..n],
            aad: path.as_bytes(),
        };
        done += n as u64;
        if n < CHUNK {
            let ct = enc
                .encrypt_last(payload)
                .map_err(|_| Error::Encode("encryption failed".into()))?;
            out.write_all(&ct)?;
            break;
        }
        let ct = enc
            .encrypt_next(payload)
            .map_err(|_| Error::Encode("encryption failed".into()))?;
        out.write_all(&ct)?;
    }
    out.sync_all()?;
    go(done);
    Ok(true)
}

/// Reads until `buf` is full or the file ends.
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

/// Decrypts a streamed object from `src` into `out`.
fn decrypt_stream(key: &Key, path: &str, mut src: impl Read, mut out: impl Write) -> Result<()> {
    let mut magic = [0u8; 4];
    let mut prefix = [0u8; STREAM_NONCE];
    src.read_exact(&mut magic).map_err(|_| crypto_err(path))?;
    src.read_exact(&mut prefix).map_err(|_| crypto_err(path))?;
    if &magic != STREAMED {
        return Err(crypto_err(path));
    }
    let mut dec = DecryptorBE32::from_aead(key.cipher(), (&prefix).into());
    let mut buf = vec![0u8; CHUNK + TAG];
    // The encryptor always ends with a short chunk (empty when the song is a whole number of
    // chunks), so a full-size chunk is never the last.
    loop {
        let n = read_full(&mut src, &mut buf)?;
        let payload = Payload {
            msg: &buf[..n],
            aad: path.as_bytes(),
        };
        if n == CHUNK + TAG {
            let plain = dec.decrypt_next(payload).map_err(|_| crypto_err(path))?;
            out.write_all(&plain)?;
        } else {
            let plain = dec.decrypt_last(payload).map_err(|_| crypto_err(path))?;
            out.write_all(&plain)?;
            return Ok(());
        }
    }
}

fn decrypt_file(key: &Key, path: &str, from: &Path, to: &Path) -> Result<()> {
    let out = fs::File::create(to)?;
    decrypt_stream(key, path, fs::File::open(from)?, &out)?;
    out.sync_all()?;
    Ok(())
}

/// Songs and artwork are uploaded as files (streamed); everything else is sealed whole.
fn streamed(path: &str) -> bool {
    path.starts_with("blobs/") || path.starts_with("art/")
}

/// A store whose objects are encrypted with the library key before they reach `inner`.
#[derive(Debug)]
pub struct EncryptedStore<S> {
    inner: S,
    key: Key,
    /// Where songs are encrypted before upload and downloaded before decryption.
    temp: PathBuf,
}

impl<S: Store> EncryptedStore<S> {
    pub fn new(inner: S, key: Key, temp: impl Into<PathBuf>) -> Self {
        Self {
            inner,
            key,
            temp: temp.into(),
        }
    }

    /// Blobs and artwork get hashed names; logs and device files keep theirs (they reveal only
    /// random device ids and counts).
    fn map(&self, path: &str) -> String {
        for top in ["blobs/", "art/"] {
            if path.starts_with(top) {
                let h = self.key.name(path);
                return format!("{top}{}/{h}", &h[..2]);
            }
        }
        path.to_owned()
    }
}

impl<S: Store> Store for EncryptedStore<S> {
    fn list(&self, dir: &str) -> Result<Vec<String>> {
        self.inner.list(dir)
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match self.inner.get(&self.map(path))? {
            None => Ok(None),
            Some(b) if b.starts_with(STREAMED) => {
                let mut out = Vec::new();
                decrypt_stream(&self.key, path, b.as_slice(), &mut out)?;
                Ok(Some(out))
            }
            Some(b) => self.key.open(&b, path).map(Some),
        }
    }

    fn put(&self, path: &str, bytes: &[u8]) -> Result<()> {
        self.inner
            .put(&self.map(path), &self.key.seal(bytes, path)?)
    }

    fn size(&self, path: &str) -> Result<Option<u64>> {
        let stored = self.inner.size(&self.map(path))?;
        Ok(if streamed(path) {
            stored.and_then(plain_size)
        } else {
            stored.and_then(|n| n.checked_sub((SEALED.len() + 24 + TAG) as u64))
        })
    }

    fn put_file(&self, path: &str, local: &Path, go: &mut dyn FnMut(u64) -> bool) -> Result<bool> {
        fs::create_dir_all(&self.temp)?;
        let tmp = self.temp.join(format!("{}.up", self.key.name(path)));
        let total = fs::metadata(local)?.len();
        // Half the progress for encrypting, half for copying.
        let ok = encrypt_file(&self.key, path, local, &tmp, &mut |n| go(n / 2))?
            && self.inner.put_file(&self.map(path), &tmp, &mut |n| {
                go(total / 2 + n.min(total) / 2)
            })?;
        let _ = fs::remove_file(&tmp);
        if ok {
            go(total);
        }
        Ok(ok)
    }

    fn get_file(&self, path: &str, dest: &Path, go: &mut dyn FnMut(u64) -> bool) -> Result<bool> {
        fs::create_dir_all(&self.temp)?;
        // The encrypted download resumes like any other; decryption happens once it's whole.
        let tmp = self.temp.join(format!("{}.down", self.key.name(path)));
        if !self.inner.get_file(&self.map(path), &tmp, go)? {
            return Ok(false);
        }
        let part = part_path(dest)?;
        let result = decrypt_file(&self.key, path, &tmp, &part);
        let _ = fs::remove_file(&tmp);
        if let Err(e) = result {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
        fs::rename(&part, dest)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{FolderStore, MemStore};

    fn key() -> (Key, Encryption) {
        Key::create_with("correct horse battery", 256, 1).expect("key")
    }

    #[test]
    fn the_right_passphrase_unlocks_and_a_wrong_one_does_not() {
        let (k, e) = key();
        let again = Key::unlock("correct horse battery", &e).expect("unlock");
        assert_eq!(again.to_bytes(), k.to_bytes());
        assert!(Key::unlock("wrong horse battery", &e).is_err());
        assert!(Key::create("short").is_err());
        assert_eq!(
            Key::from_bytes(&k.to_bytes()).map(|k| k.to_bytes()),
            Some(k.to_bytes())
        );
    }

    #[test]
    fn small_objects_round_trip_and_are_bound_to_their_paths() {
        let (k, _) = key();
        let mem = MemStore::new();
        let store = EncryptedStore::new(mem, k.clone(), std::env::temp_dir());
        store.put("log/a/0.seg", b"hello ops").expect("put");
        assert_eq!(
            store.get("log/a/0.seg").expect("get").as_deref(),
            Some(&b"hello ops"[..])
        );
        let raw = store.inner.get("log/a/0.seg").expect("raw").expect("there");
        assert!(!raw.windows(5).any(|w| w == b"hello"), "stored encrypted");
        // Moved to another path, it no longer opens.
        store.inner.put("log/a/1.seg", &raw).expect("put");
        assert!(store.get("log/a/1.seg").is_err());
        // Artwork names are hidden.
        store.put("art/ab/abcdef", b"img").expect("put");
        assert!(store.inner.get("art/ab/abcdef").expect("raw").is_none());
        assert_eq!(
            store.get("art/ab/abcdef").expect("get").as_deref(),
            Some(&b"img"[..])
        );
    }

    #[test]
    fn songs_round_trip_in_chunks_with_resumable_downloads() {
        let (k, _) = key();
        let dir = tempfile::tempdir().expect("tempdir");
        let store = EncryptedStore::new(
            FolderStore::new(dir.path().join("store")),
            k,
            dir.path().join("tmp"),
        );
        for len in [
            0,
            10,
            CHUNK - 1,
            CHUNK,
            CHUNK + 1,
            2 * CHUNK,
            2 * CHUNK + 5,
            3 * CHUNK,
        ] {
            let song: Vec<u8> = (0..len).map(|i| (i * 7 % 253) as u8).collect();
            let src = dir.path().join("song.flac");
            fs::write(&src, &song).expect("write");
            let path = format!("blobs/aa/aa{len}.flac");
            assert!(store.put_file(&path, &src, &mut |_| true).expect("put"));
            assert_eq!(store.size(&path).expect("size"), Some(len as u64), "{len}");
            let dest = dir.path().join(format!("out{len}.flac"));
            assert!(store.get_file(&path, &dest, &mut |_| true).expect("get"));
            assert_eq!(fs::read(&dest).expect("read"), song, "{len}");
        }
        // An image uploaded as a file reads back whole.
        let img = dir.path().join("cover.jpg");
        fs::write(&img, b"jpeg bytes").expect("write");
        assert!(
            store
                .put_file("art/ab/abcd", &img, &mut |_| true)
                .expect("put")
        );
        assert_eq!(
            store.get("art/ab/abcd").expect("get").as_deref(),
            Some(&b"jpeg bytes"[..])
        );
        assert_eq!(store.size("art/ab/abcd").expect("size"), Some(10));
        store.put("log/x/0.seg", b"12345").expect("put");
        assert_eq!(store.size("log/x/0.seg").expect("size"), Some(5));
        // Names in the folder say nothing about the songs.
        let names: Vec<String> = fs::read_dir(dir.path().join("store/blobs"))
            .expect("dir")
            .flat_map(|d| fs::read_dir(d.expect("e").path()).expect("dir"))
            .map(|e| e.expect("e").file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names
                .iter()
                .all(|n| !n.contains("flac") && !n.starts_with("aa")),
            "{names:?}"
        );
        assert!(
            !dir.path()
                .join("tmp")
                .read_dir()
                .expect("tmp")
                .any(|_| true),
            "temp files go"
        );
    }
}
