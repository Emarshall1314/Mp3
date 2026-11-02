//! Paired devices talking over the local network (D64).
//!
//! Each device runs a small HTTP/1.1 server and keeps its own store (its log, copies of the other
//! devices' logs, device and handoff files, artwork). A round of sync copies, from each paired
//! device it can reach, whatever that device's store has and this one's lacks ([`pull`]); after
//! that everything works as if the devices shared one folder. Songs aren't copied into the store:
//! a device asks another for one and gets it straight from that device's music folder.
//!
//! - `GET /hello`: who this is and which library it's in (the key derivation's public part, so
//!   a device joining can check the pairing code). Plain JSON, the only thing sent unencrypted.
//! - `GET /tree`: every file in the store but songs, with sizes.
//! - `GET /file?path=…`: one of them.
//! - `GET /blob?path=blobs/…`: a song, streamed.
//! - `POST /poke`: the sender has news; sync soon.
//!
//! Everything but `/hello` needs a request signed with the library key, and every answer is
//! sealed with it (bound to the path asked for), so a device on the same Wi-Fi without the key
//! gets nothing and learns nothing.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::crypt::{Encryption, Key, decrypt_stream, encrypt_stream};
use crate::store::{FolderStore, Store, part_path};
use crate::{Error, Result};

/// The port a device listens on when it's free (another is taken otherwise); fixed so that an
/// address can be typed in when discovery doesn't work on a network.
pub const PORT: u16 = 47100;
/// How far a request's time may be from this device's clock.
const SKEW_S: u64 = 300;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// Connections served at once; more wait (a device has few peers).
const MAX_CONNECTIONS: usize = 16;

/// What a device says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub device: String,
    pub name: String,
    pub platform: String,
    /// The library it syncs, if any: how the key is derived from the pairing code.
    pub library: Option<Encryption>,
}

impl Hello {
    /// Short public id of the library, the same on every device in it.
    pub fn library_id(&self) -> Option<String> {
        self.library.as_ref().map(library_id)
    }
}

/// A library's public id: the start of its key derivation's salt.
pub fn library_id(e: &Encryption) -> String {
    e.salt.chars().take(16).collect()
}

/// What a device serving others provides.
pub trait Host: Send + Sync + 'static {
    fn hello(&self) -> Hello;
    /// The library key; none when not paired (then only `/hello` answers).
    fn key(&self) -> Option<Key>;
    /// The folder of this device's store.
    fn store_root(&self) -> PathBuf;
    /// The song file for a `blobs/…` path, if this device has it and shares its songs.
    fn blob(&self, path: &str) -> Option<PathBuf>;
    /// A paired device made a request: where it can be reached, and whether it asked for a sync.
    fn seen(&self, device: &str, at: SocketAddr, poked: bool);
}

/// The server, running until dropped.
#[derive(Debug)]
pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Listens on [`PORT`] if it's free, else on any port.
    pub fn start(host: Arc<dyn Host>) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT))
            .or_else(|_| TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)))?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let quit = Arc::clone(&stop);
        let busy = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new()
            .name("cadenza-peer".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    if quit.load(Ordering::Acquire) {
                        return;
                    }
                    let Ok(conn) = conn else { continue };
                    if busy.load(Ordering::Acquire) >= MAX_CONNECTIONS {
                        continue;
                    }
                    busy.fetch_add(1, Ordering::AcqRel);
                    let (host, done) = (Arc::clone(&host), Arc::clone(&busy));
                    let spawned = std::thread::Builder::new()
                        .name("cadenza-peer-conn".into())
                        .spawn(move || {
                            let _ = serve(host.as_ref(), conn);
                            done.fetch_sub(1, Ordering::AcqRel);
                        });
                    if spawned.is_err() {
                        busy.fetch_sub(1, Ordering::AcqRel);
                    }
                }
            })?;
        Ok(Self { port, stop })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the accept loop so it sees the flag.
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)),
            Duration::from_millis(200),
        );
    }
}

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What a signature covers.
fn signed(method: &str, target: &str, device: &str, port: u16, time: u64) -> Vec<u8> {
    format!("{method} {target}\n{device}\n{port}\n{time}").into_bytes()
}

/// Paths a peer may ask for: relative, `/`-separated, plain names. Device ids, hashes and
/// sequence numbers need nothing else, so nothing needs escaping in a URL either.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() < 512
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._:-".contains(&b))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

struct Request {
    method: String,
    target: String,
    path: String,
    query: BTreeMap<String, String>,
    headers: BTreeMap<String, String>,
}

fn read_request(reader: &mut impl BufRead) -> Option<Request> {
    let mut line = String::new();
    reader.by_ref().take(4096).read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let mut headers = BTreeMap::new();
    for _ in 0..32 {
        let mut h = String::new();
        reader.by_ref().take(4096).read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }
    }
    let (path, q) = target.split_once('?').unwrap_or((&target, ""));
    let query = q
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    Some(Request {
        method,
        path: path.to_owned(),
        target: target.clone(),
        query,
        headers,
    })
}

fn respond(conn: &mut impl Write, status: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        conn,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    conn.write_all(body)?;
    conn.flush()
}

/// Every file in the store but songs, with sizes, `/`-separated.
fn tree(root: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            // Temporary and partial files start with a dot.
            if name.starts_with('.') || name.ends_with(".tmp") {
                continue;
            }
            let path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if path == "blobs" {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push((e.path(), path));
            } else if safe_path(&path) {
                out.push((path, meta.len()));
            }
        }
    }
    out.sort();
    out
}

fn serve(host: &dyn Host, conn: TcpStream) -> std::io::Result<()> {
    conn.set_read_timeout(Some(IO_TIMEOUT))?;
    conn.set_write_timeout(Some(IO_TIMEOUT))?;
    let peer_ip = conn.peer_addr()?.ip();
    let mut reader = BufReader::new(conn.try_clone()?);
    let mut conn = conn;
    let Some(req) = read_request(&mut reader) else {
        return respond(&mut conn, "400 Bad Request", b"");
    };
    if req.method == "GET" && req.path == "/hello" {
        let body = serde_json::to_vec(&host.hello()).unwrap_or_default();
        return respond(&mut conn, "200 OK", &body);
    }
    let Some(key) = host.key() else {
        return respond(&mut conn, "403 Forbidden", b"not paired");
    };
    let header = |k: &str| req.headers.get(k).map(String::as_str).unwrap_or("");
    let device = header("x-cadenza-device").to_owned();
    let port: u16 = header("x-cadenza-port").parse().unwrap_or(0);
    let time: u64 = header("x-cadenza-time").parse().unwrap_or(0);
    let message = signed(&req.method, &req.target, &device, port, time);
    if device.is_empty()
        || now_s().abs_diff(time) > SKEW_S
        || !key.verify(&message, header("x-cadenza-sig"))
    {
        return respond(&mut conn, "401 Unauthorized", b"");
    }
    let poke = req.method == "POST" && req.path == "/poke";
    host.seen(&device, SocketAddr::new(peer_ip, port), poke);
    let path = req.query.get("path").map(String::as_str).unwrap_or("");
    let root = host.store_root();
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/poke") => respond(&mut conn, "204 No Content", b""),
        ("GET", "/tree") => {
            let body = serde_json::to_vec(&tree(&root)).unwrap_or_default();
            match key.seal(&body, "tree") {
                Ok(sealed) => respond(&mut conn, "200 OK", &sealed),
                Err(_) => respond(&mut conn, "500 Internal Server Error", b""),
            }
        }
        ("GET", "/file") if safe_path(path) && !path.starts_with("blobs/") => {
            match FolderStore::new(&root).get(path) {
                Ok(Some(bytes)) => match key.seal(&bytes, path) {
                    Ok(sealed) => respond(&mut conn, "200 OK", &sealed),
                    Err(_) => respond(&mut conn, "500 Internal Server Error", b""),
                },
                _ => respond(&mut conn, "404 Not Found", b""),
            }
        }
        ("GET", "/blob") if safe_path(path) && path.starts_with("blobs/") => {
            let Some(file) = host.blob(path).and_then(|p| std::fs::File::open(p).ok()) else {
                return respond(&mut conn, "404 Not Found", b"");
            };
            // Streamed to the end of the connection: no length, as encryption adds a little.
            write!(conn, "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")?;
            let mut out = std::io::BufWriter::with_capacity(1 << 16, &mut conn);
            encrypt_stream(&key, path, file, &mut out, &mut |_| true)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            out.flush()
        }
        _ => respond(&mut conn, "404 Not Found", b""),
    }
}

/// This device as it signs its requests to the others.
#[derive(Debug, Clone)]
pub struct Me {
    pub device: String,
    /// The port this device's own server listens on, so the other can reach it back.
    pub port: u16,
    pub key: Key,
}

/// Another device in the library, at an address.
#[derive(Debug, Clone)]
pub struct Peer {
    pub addr: SocketAddr,
    me: Me,
}

/// Parses "host:port", "host" (the usual port) or "[v6]:port"; looks names up.
pub fn parse_address(text: &str) -> Option<SocketAddr> {
    let text = text.trim();
    if let Ok(a) = text.parse::<SocketAddr>() {
        return Some(a);
    }
    if let Ok(ip) = text.parse::<IpAddr>() {
        return Some(SocketAddr::new(ip, PORT));
    }
    let with_port = if text.contains(':') {
        text.to_owned()
    } else {
        format!("{text}:{PORT}")
    };
    with_port.to_socket_addrs().ok()?.next()
}

fn connect(addr: SocketAddr) -> Result<TcpStream> {
    let conn = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    conn.set_read_timeout(Some(IO_TIMEOUT))?;
    conn.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(conn)
}

/// Reads a response's status line and headers; the body follows in the reader.
fn read_head(reader: &mut impl BufRead) -> Result<(u16, Option<usize>)> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Error::Segment("not a Cadenza device".into()))?;
    let mut length = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':')
            && k.trim().eq_ignore_ascii_case("content-length")
        {
            length = v.trim().parse().ok();
        }
    }
    Ok((status, length))
}

fn read_body(reader: &mut impl Read, length: Option<usize>) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    match length {
        Some(n) => {
            body.resize(n, 0);
            reader.read_exact(&mut body)?;
        }
        None => {
            reader.read_to_end(&mut body)?;
        }
    }
    Ok(body)
}

/// Asks a device who it is (no key needed).
pub fn hello(addr: SocketAddr) -> Result<Hello> {
    let mut conn = connect(addr)?;
    write!(
        conn,
        "GET /hello HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )?;
    let mut reader = BufReader::new(conn);
    let (status, length) = read_head(&mut reader)?;
    if status != 200 {
        return Err(Error::Segment(format!("the device answered {status}")));
    }
    let body = read_body(&mut reader, length)?;
    serde_json::from_slice(&body).map_err(|e| Error::Segment(format!("not a Cadenza device: {e}")))
}

impl Peer {
    pub fn new(addr: SocketAddr, me: Me) -> Self {
        Self { addr, me }
    }

    /// Sends a signed request; the status and the rest of the response.
    fn request(
        &self,
        method: &str,
        target: &str,
    ) -> Result<(u16, BufReader<TcpStream>, Option<usize>)> {
        let mut conn = connect(self.addr)?;
        let time = now_s();
        let sig = self
            .me
            .key
            .mac(&signed(method, target, &self.me.device, self.me.port, time));
        write!(
            conn,
            "{method} {target} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\
             X-Cadenza-Device: {}\r\nX-Cadenza-Port: {}\r\nX-Cadenza-Time: {time}\r\n\
             X-Cadenza-Sig: {sig}\r\nContent-Length: 0\r\n\r\n",
            self.addr, self.me.device, self.me.port
        )?;
        let mut reader = BufReader::new(conn);
        let (status, length) = read_head(&mut reader)?;
        match status {
            401 => Err(Error::Crypto(
                "the other device turned the request down: it's in a different library, or its \
                 clock is more than five minutes out"
                    .into(),
            )),
            403 => Err(Error::Crypto("the other device isn't syncing".into())),
            _ => Ok((status, reader, length)),
        }
    }

    /// Every file in its store but songs, with sizes.
    pub fn tree(&self) -> Result<Vec<(String, u64)>> {
        let (status, mut reader, length) = self.request("GET", "/tree")?;
        if status != 200 {
            return Err(Error::Segment(format!("listing failed ({status})")));
        }
        let plain = self.me.key.open(&read_body(&mut reader, length)?, "tree")?;
        serde_json::from_slice(&plain).map_err(|e| Error::Segment(e.to_string()))
    }

    /// One file from its store, if it has it.
    pub fn file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        if !safe_path(path) {
            return Err(Error::Path(path.into()));
        }
        let (status, mut reader, length) = self.request("GET", &format!("/file?path={path}"))?;
        match status {
            200 => Ok(Some(
                self.me.key.open(&read_body(&mut reader, length)?, path)?,
            )),
            404 => Ok(None),
            s => Err(Error::Segment(format!("reading {path} failed ({s})"))),
        }
    }

    /// Downloads a song to `dest` through a hidden `.part` file, renamed into place when it's
    /// complete and genuine. False if `go` stopped it; an error if the device doesn't have it.
    pub fn blob_to(
        &self,
        path: &str,
        dest: &Path,
        go: &mut dyn FnMut(u64) -> bool,
    ) -> Result<bool> {
        if !safe_path(path) || !path.starts_with("blobs/") {
            return Err(Error::Path(path.into()));
        }
        let (status, reader, _) = self.request("GET", &format!("/blob?path={path}"))?;
        if status != 200 {
            return Err(Error::Path(format!("{path} isn't on that device")));
        }
        let part = part_path(dest)?;
        let out = std::fs::File::create(&part)?;
        let mut writer = std::io::BufWriter::with_capacity(1 << 16, &out);
        let result = decrypt_stream(&self.me.key, path, reader, &mut writer, go);
        drop(writer);
        match result {
            Ok(true) => {
                out.sync_all()?;
                std::fs::rename(&part, dest)?;
                Ok(true)
            }
            other => {
                let _ = std::fs::remove_file(&part);
                other
            }
        }
    }

    /// Tells it this device has news.
    pub fn poke(&self) -> Result<()> {
        self.request("POST", "/poke").map(|_| ())
    }
}

/// What [`pull`] copied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pulled {
    /// New log segments.
    pub segments: usize,
    /// Everything else (device and handoff files, artwork).
    pub other: usize,
}

/// Copies what `peer`'s store has and `store` lacks: others' log segments and artwork once
/// (they never change), and the peer's own device and handoff files whenever they differ.
/// This device's own files are never taken from another. `peer_id` is the peer's device id.
pub fn pull(peer: &Peer, store: &FolderStore, me: &str, peer_id: &str) -> Result<Pulled> {
    let mut pulled = Pulled::default();
    let mine = |p: &str| {
        p.starts_with(&format!("log/{me}/"))
            || p == format!("devices/{me}.json")
            || p == format!("handoff/{me}.json")
    };
    let owned_by_peer =
        |p: &str| p == format!("devices/{peer_id}.json") || p == format!("handoff/{peer_id}.json");
    for (path, size) in peer.tree()? {
        let wanted = path.starts_with("log/")
            || path.starts_with("devices/")
            || path.starts_with("handoff/")
            || path.starts_with("art/");
        if !wanted || mine(&path) {
            continue;
        }
        // Its own files change in place (they're small); everything else is written once.
        let have = store.size(&path)?;
        if have.is_some() && !owned_by_peer(&path) {
            continue;
        }
        let Some(bytes) = peer.file(&path)? else {
            continue;
        };
        if have == Some(size) && store.get(&path)?.as_deref() == Some(bytes.as_slice()) {
            continue;
        }
        store.put(&path, &bytes)?;
        if path.starts_with("log/") {
            pulled.segments += 1;
        } else {
            pulled.other += 1;
        }
    }
    Ok(pulled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct TestHost {
        hello: Hello,
        key: Option<Key>,
        root: PathBuf,
        song: PathBuf,
        seen: Mutex<Vec<(String, bool)>>,
    }

    impl Host for TestHost {
        fn hello(&self) -> Hello {
            self.hello.clone()
        }
        fn key(&self) -> Option<Key> {
            self.key.clone()
        }
        fn store_root(&self) -> PathBuf {
            self.root.clone()
        }
        fn blob(&self, path: &str) -> Option<PathBuf> {
            (path == "blobs/ab/abcd.flac").then(|| self.song.clone())
        }
        fn seen(&self, device: &str, _at: SocketAddr, poked: bool) {
            self.seen
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((device.into(), poked));
        }
    }

    #[test]
    fn paths_stay_inside_the_store() {
        assert!(safe_path("log/0193-ab/12.seg"));
        assert!(safe_path("blobs/ab/abcd:2.flac"));
        assert!(!safe_path("../secret"));
        assert!(!safe_path("log//x"));
        assert!(!safe_path("/etc/passwd"));
        assert!(!safe_path("log/a b"));
        assert!(!safe_path("art/./x"));
        assert_eq!(
            parse_address("192.168.1.20"),
            Some(SocketAddr::from(([192, 168, 1, 20], PORT)))
        );
        assert_eq!(
            parse_address(" 10.0.0.2:5000 "),
            Some(SocketAddr::from(([10, 0, 0, 2], 5000)))
        );
    }

    #[test]
    fn a_paired_device_copies_what_it_lacks_and_strangers_get_nothing() {
        let (key, encryption) = Key::create_with("ABCD-EFGH-JKLM", 256, 1).expect("key");
        let dir = tempfile::tempdir().expect("tempdir");
        let a_root = dir.path().join("a");
        let a = FolderStore::new(&a_root);
        a.put("log/devA/0.seg", b"segment zero").expect("put");
        a.put("log/devA/1.seg", b"segment one").expect("put");
        a.put("log/devB/0.seg", b"B's own, never taken back")
            .expect("put");
        a.put("devices/devA.json", b"{\"name\":\"A\"}")
            .expect("put");
        a.put("art/ab/abcdef", b"jpeg").expect("put");
        let song: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        let song_path = dir.path().join("song.flac");
        std::fs::write(&song_path, &song).expect("write");
        let host = Arc::new(TestHost {
            hello: Hello {
                device: "devA".into(),
                name: "A".into(),
                platform: "linux".into(),
                library: Some(encryption.clone()),
            },
            key: Some(key.clone()),
            root: a_root,
            song: song_path,
            seen: Mutex::new(Vec::new()),
        });
        let server = Server::start(Arc::clone(&host) as Arc<dyn Host>).expect("server");
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, server.port()));

        // Anyone may ask who it is; the code turns that into the key.
        let h = hello(addr).expect("hello");
        assert_eq!(h.device, "devA");
        let joined =
            Key::unlock("ABCD-EFGH-JKLM", h.library.as_ref().expect("library")).expect("unlock");

        let b = FolderStore::new(dir.path().join("b"));
        b.put("log/devB/0.seg", b"B's own").expect("put");
        let peer = Peer::new(
            addr,
            Me {
                device: "devB".into(),
                port: 4242,
                key: joined,
            },
        );
        let got = pull(&peer, &b, "devB", "devA").expect("pull");
        assert_eq!(
            got,
            Pulled {
                segments: 2,
                other: 2
            }
        );
        assert_eq!(
            b.get("log/devA/1.seg").expect("get").as_deref(),
            Some(&b"segment one"[..])
        );
        assert_eq!(
            b.get("log/devB/0.seg").expect("get").as_deref(),
            Some(&b"B's own"[..])
        );
        // A second pull copies nothing.
        assert_eq!(
            pull(&peer, &b, "devB", "devA").expect("pull"),
            Pulled::default()
        );
        // The peer's own files follow its changes.
        a.put("devices/devA.json", b"{\"name\":\"A2\"}")
            .expect("put");
        assert_eq!(pull(&peer, &b, "devB", "devA").expect("pull").other, 1);

        // A song, streamed and checked.
        let dest = dir.path().join("got.flac");
        assert!(
            peer.blob_to("blobs/ab/abcd.flac", &dest, &mut |_| true)
                .expect("blob")
        );
        assert_eq!(std::fs::read(&dest).expect("read"), song);
        assert!(
            peer.blob_to(
                "blobs/ab/other.flac",
                &dir.path().join("x.flac"),
                &mut |_| true
            )
            .is_err()
        );
        assert!(!dir.path().join(".x.flac.part").exists());
        peer.poke().expect("poke");
        let seen = host.seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert!(seen.contains(&("devB".into(), true)));

        // A device with another key is refused.
        let (stranger_key, _) = Key::create_with("ZZZZ-ZZZZ-ZZZZ", 256, 1).expect("key");
        let stranger = Peer::new(
            addr,
            Me {
                device: "devC".into(),
                port: 1,
                key: stranger_key,
            },
        );
        assert!(stranger.tree().is_err());
        assert!(stranger.file("log/devA/0.seg").is_err());
    }
}
