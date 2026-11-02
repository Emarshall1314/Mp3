//! Encryption between paired devices (D64): everything that crosses the network is sealed with
//! the library key, so another device on the same Wi-Fi sees only opaque data and can't ask for
//! anything without the key.
//!
//! - The key comes from the pairing code with Argon2id. Its salt and cost, and a check value
//!   that tells a wrong code from a right one, are public (a device that wants to join reads them
//!   before the owner types the code).
//! - Small objects (log segments, device files, artwork, lists) are sealed whole with
//!   XChaCha20-Poly1305: `CDZE`, a random 24-byte nonce, then the ciphertext.
//! - Songs are encrypted in 1 MiB chunks (the AEAD "STREAM" construction) so they never have to
//!   be in memory at once: `CDZF`, a random 19-byte nonce prefix, then the chunks.
//! - Every object is bound to its path (associated data), so objects can't be swapped around.
//! - Requests are signed with a keyed hash, so a device without the key is refused.

use std::io::{Read, Write};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::stream::{DecryptorBE32, EncryptorBE32};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload, rand_core::RngCore};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{Error, Result};

const SEALED: &[u8; 4] = b"CDZE";
const STREAMED: &[u8; 4] = b"CDZF";
const CHUNK: usize = 1 << 20;
const TAG: usize = 16;
const STREAM_NONCE: usize = 19;
const CHECK: &[u8] = b"cadenza library key check";

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

    /// `plain` sealed whole, bound to `path`.
    pub fn seal(&self, plain: &[u8], path: &str) -> Result<Vec<u8>> {
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

    /// What [`seal`](Self::seal) sealed for the same `path`, if it is genuine.
    pub fn open(&self, sealed: &[u8], path: &str) -> Result<Vec<u8>> {
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

    /// A keyed hash of `message`, hex: signs requests, and names the library without revealing
    /// the key.
    pub fn mac(&self, message: &[u8]) -> String {
        let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(&self.names) else {
            return String::new();
        };
        mac.update(message);
        hex(&mac.finalize().into_bytes())
    }

    /// Whether `tag` is [`mac`](Self::mac) of `message`, compared in constant time.
    pub fn verify(&self, message: &[u8], tag: &str) -> bool {
        let Some(tag) = unhex(tag) else { return false };
        let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(&self.names) else {
            return false;
        };
        mac.update(message);
        mac.verify_slice(&tag).is_ok()
    }
}

/// The letters a pairing code is made of: no 0/O or 1/I to confuse.
const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// A new pairing code, `XXXX-XXXX-XXXX`: 60 random bits, which with Argon2id is far beyond
/// guessing from what crosses the network.
pub fn pairing_code() -> String {
    let mut bytes = [0u8; 12];
    OsRng.fill_bytes(&mut bytes);
    let letters: String = bytes
        .iter()
        .map(|b| char::from(CODE_ALPHABET[usize::from(b % 32)]))
        .collect();
    format!("{}-{}-{}", &letters[..4], &letters[4..8], &letters[8..])
}

/// A pairing code as typed (any case, with spaces, dashes or neither) in the form it was made
/// in; none if it can't be one.
pub fn normalize_code(typed: &str) -> Option<String> {
    let letters: String = typed
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (letters.len() == 12 && letters.bytes().all(|b| CODE_ALPHABET.contains(&b)))
        .then(|| format!("{}-{}-{}", &letters[..4], &letters[4..8], &letters[8..]))
}

/// Encrypts `src` onto `out` in chunks, bound to `path`; `go` hears the plaintext bytes done and
/// returns false to stop (the output is then incomplete and won't decrypt). False if stopped.
pub fn encrypt_stream(
    key: &Key,
    path: &str,
    mut src: impl Read,
    mut out: impl Write,
    go: &mut dyn FnMut(u64) -> bool,
) -> Result<bool> {
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
    out.flush()?;
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

/// Decrypts what [`encrypt_stream`] wrote for the same `path` from `src` onto `out`; `go` hears
/// the plaintext bytes done and returns false to stop. False if stopped; an error if anything
/// isn't genuine (so a partly written `out` must be thrown away).
pub fn decrypt_stream(
    key: &Key,
    path: &str,
    mut src: impl Read,
    mut out: impl Write,
    go: &mut dyn FnMut(u64) -> bool,
) -> Result<bool> {
    let mut magic = [0u8; 4];
    let mut prefix = [0u8; STREAM_NONCE];
    src.read_exact(&mut magic).map_err(|_| crypto_err(path))?;
    src.read_exact(&mut prefix).map_err(|_| crypto_err(path))?;
    if &magic != STREAMED {
        return Err(crypto_err(path));
    }
    let mut dec = DecryptorBE32::from_aead(key.cipher(), (&prefix).into());
    let mut buf = vec![0u8; CHUNK + TAG];
    let mut done = 0u64;
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
            done += plain.len() as u64;
            if !go(done) {
                return Ok(false);
            }
        } else {
            let plain = dec.decrypt_last(payload).map_err(|_| crypto_err(path))?;
            out.write_all(&plain)?;
            out.flush()?;
            go(done + plain.len() as u64);
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let sealed = k.seal(b"hello ops", "log/a/0.seg").expect("seal");
        assert!(!sealed.windows(5).any(|w| w == b"hello"), "sealed");
        assert_eq!(k.open(&sealed, "log/a/0.seg").expect("open"), b"hello ops");
        // Moved to another path, it no longer opens.
        assert!(k.open(&sealed, "log/a/1.seg").is_err());
        let (other, _) = key();
        assert!(other.open(&sealed, "log/a/0.seg").is_err());
    }

    #[test]
    fn songs_round_trip_in_chunks() {
        let (k, _) = key();
        for len in [0, 10, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK, 2 * CHUNK + 5] {
            let song: Vec<u8> = (0..len).map(|i| (i * 7 % 253) as u8).collect();
            let path = format!("blobs/aa/aa{len}.flac");
            let mut wire = Vec::new();
            assert!(
                encrypt_stream(&k, &path, song.as_slice(), &mut wire, &mut |_| true).expect("enc")
            );
            let mut back = Vec::new();
            let mut last = 0;
            assert!(
                decrypt_stream(&k, &path, wire.as_slice(), &mut back, &mut |d| {
                    last = d;
                    true
                })
                .expect("dec")
            );
            assert_eq!(back, song, "{len}");
            assert_eq!(last, len as u64);
            // Cut short or tampered with, it fails rather than giving part of a song.
            if len > 0 {
                let mut cut = wire.clone();
                cut.truncate(wire.len() - 1);
                assert!(
                    decrypt_stream(&k, &path, cut.as_slice(), Vec::new(), &mut |_| true).is_err()
                );
                assert!(
                    decrypt_stream(
                        &k,
                        "blobs/aa/other.flac",
                        wire.as_slice(),
                        Vec::new(),
                        &mut |_| true
                    )
                    .is_err()
                );
            }
        }
    }

    #[test]
    fn pairing_codes_read_back_however_they_are_typed() {
        let code = pairing_code();
        assert_eq!(code.len(), 14);
        assert_eq!(normalize_code(&code).as_deref(), Some(code.as_str()));
        assert_eq!(
            normalize_code(&code.to_lowercase().replace('-', " ")).as_deref(),
            Some(code.as_str())
        );
        assert_ne!(pairing_code(), code);
        assert_eq!(normalize_code("ABCD-EFGH-JKL"), None);
    }

    #[test]
    fn requests_are_signed() {
        let (k, _) = key();
        let tag = k.mac(b"GET /tree");
        assert!(k.verify(b"GET /tree", &tag));
        assert!(!k.verify(b"GET /tree?x", &tag));
        let (other, _) = key();
        assert!(!other.verify(b"GET /tree", &tag));
    }
}
