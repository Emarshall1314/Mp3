//! Finding other Cadenza devices on the network with mDNS (D64): each device advertises
//! `_cadenza._tcp` with its id, name, platform and library id, and browses for the others. A
//! device found here is only talked to once it's in the same library (see [`crate::peer`]).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::peer::Hello;
use crate::{Error, Result};

const SERVICE: &str = "_cadenza._tcp.local.";

/// Another device seen on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub device: String,
    pub name: String,
    pub platform: String,
    /// The library it's in ([`crate::peer::library_id`]), if any.
    pub library: Option<String>,
    pub addrs: Vec<SocketAddr>,
}

pub struct Discovery {
    daemon: ServiceDaemon,
    found: Arc<Mutex<HashMap<String, Found>>>,
    me: Mutex<Option<String>>,
}

impl std::fmt::Debug for Discovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Discovery").finish_non_exhaustive()
    }
}

fn err(e: mdns_sd::Error) -> Error {
    Error::Io(std::io::Error::other(e.to_string()))
}

impl Discovery {
    /// Starts browsing; [`advertise`](Self::advertise) makes this device visible.
    pub fn start() -> Result<Self> {
        let daemon = ServiceDaemon::new().map_err(err)?;
        let events = daemon.browse(SERVICE).map_err(err)?;
        let found: Arc<Mutex<HashMap<String, Found>>> = Arc::default();
        let into = Arc::clone(&found);
        std::thread::Builder::new()
            .name("cadenza-mdns".into())
            .spawn(move || {
                while let Ok(event) = events.recv() {
                    let Ok(mut found) = into.lock() else { return };
                    match event {
                        ServiceEvent::ServiceResolved(s) => {
                            let prop = |k: &str| {
                                s.txt_properties
                                    .get_property_val_str(k)
                                    .unwrap_or("")
                                    .to_owned()
                            };
                            let mut addrs: Vec<SocketAddr> = s
                                .addresses
                                .iter()
                                .map(|a| SocketAddr::new(a.to_ip_addr(), s.port))
                                .filter(|a| !a.ip().is_loopback())
                                .collect();
                            // IPv4 first: more often reachable on home networks.
                            addrs.sort_by_key(|a| (a.is_ipv6(), *a));
                            let library = Some(prop("lib")).filter(|l| !l.is_empty());
                            found.insert(
                                s.fullname.clone(),
                                Found {
                                    device: prop("id"),
                                    name: prop("name"),
                                    platform: prop("platform"),
                                    library,
                                    addrs,
                                },
                            );
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            found.remove(&fullname);
                        }
                        _ => {}
                    }
                }
            })?;
        Ok(Self {
            daemon,
            found,
            me: Mutex::new(None),
        })
    }

    /// Advertises this device (again, when its name or library changed) on `port`.
    pub fn advertise(&self, hello: &Hello, port: u16) -> Result<()> {
        let library = hello.library_id().unwrap_or_default();
        let props = [
            ("id", hello.device.as_str()),
            ("name", hello.name.as_str()),
            ("platform", hello.platform.as_str()),
            ("lib", library.as_str()),
        ];
        // Instance and host names from the device id: unique, and nothing personal.
        let short: String = hello
            .device
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(12)
            .collect();
        let info = ServiceInfo::new(
            SERVICE,
            &format!("cadenza-{short}"),
            &format!("cadenza-{short}.local."),
            "",
            port,
            &props[..],
        )
        .map_err(err)?
        .enable_addr_auto();
        let mut me = self.me.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = me.take() {
            let _ = self.daemon.unregister(&old);
        }
        let fullname = info.get_fullname().to_owned();
        self.daemon.register(info).map_err(err)?;
        *me = Some(fullname);
        Ok(())
    }

    /// The other devices seen, by device id.
    pub fn found(&self, me: &str) -> Vec<Found> {
        let found = self.found.lock().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<Found> = found
            .values()
            .filter(|f| !f.device.is_empty() && f.device != me && !f.addrs.is_empty())
            .cloned()
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.device.cmp(&b.device)));
        out.dedup_by(|a, b| a.device == b.device);
        out
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}
