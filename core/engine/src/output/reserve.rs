//! Audio device reservation (`org.freedesktop.ReserveDevice1`), the D-Bus protocol PipeWire,
//! PulseAudio and JACK use to hand a sound card to one program.
//!
//! Before opening `hw:N` exclusively we own the bus name `org.freedesktop.ReserveDevice1.AudioN`.
//! If the sound server holds it, we ask it to release the card (`RequestRelease`); the server then
//! closes the device. We keep the name until the stream closes, and refuse release requests
//! meanwhile. Dropping the [`Reservation`] gives the name back, and the server takes the card again.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply;
use dbus::channel::MatchingReceiver;
use dbus::message::MatchRule;

use crate::{Error, Result};

const INTERFACE: &str = "org.freedesktop.ReserveDevice1";
/// Higher than the sound servers' defaults; the maximum would mean "never ask me".
const PRIORITY: i32 = i32::MAX - 1;

#[derive(Debug)]
pub struct Reservation {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Reservation {
    /// Reserves ALSA card `card`. `Ok(None)` when there is no session bus (no desktop session,
    /// so no sound server to coordinate with).
    pub fn acquire(card: i32) -> Result<Option<Self>> {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("cadenza-reserve".into())
            .spawn(move || serve(card, &tx, &stop_flag))
            .map_err(|e| Error::Output(e.to_string()))?;
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(true)) => Ok(Some(Self {
                stop,
                thread: Some(thread),
            })),
            Ok(Ok(false)) => {
                let _ = thread.join();
                Ok(None)
            }
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                Err(Error::Output("device reservation timed out".into()))
            }
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Owns the connection: takes the name, reports, then answers requests until told to stop.
fn serve(card: i32, report: &mpsc::Sender<Result<bool>>, stop: &AtomicBool) {
    let Ok(conn) = Connection::new_session() else {
        let _ = report.send(Ok(false));
        return;
    };
    let name = format!("{INTERFACE}.Audio{card}");
    let path = format!("/org/freedesktop/ReserveDevice1/Audio{card}");
    match take_name(&conn, &name, &path) {
        Ok(()) => {}
        Err(e) => {
            let _ = report.send(Err(e));
            return;
        }
    }
    // Refuse to give the card up while we play from it.
    conn.start_receive(
        MatchRule::new_method_call()
            .with_interface(INTERFACE)
            .with_member("RequestRelease"),
        Box::new(|msg, conn| {
            let _ = dbus::channel::Sender::send(conn, msg.method_return().append1(false));
            true
        }),
    );
    let _ = report.send(Ok(true));
    while !stop.load(Ordering::Acquire) {
        let _ = conn.process(Duration::from_millis(100));
    }
    let _ = conn.release_name(name.as_str());
}

fn take_name(conn: &Connection, name: &str, path: &str) -> Result<()> {
    let busy = |what: String| Error::Output(format!("device reservation: {what}"));
    let reply = conn
        .request_name(name, false, false, true)
        .map_err(|e| busy(e.to_string()))?;
    if reply == RequestNameReply::PrimaryOwner {
        return Ok(());
    }
    // Someone holds the card: ask them to let go.
    let owner = conn.with_proxy(name, path, Duration::from_secs(3));
    let (released,): (bool,) = owner
        .method_call(INTERFACE, "RequestRelease", (PRIORITY,))
        .map_err(|e| busy(e.to_string()))?;
    if !released {
        return Err(Error::Output(
            "the device is reserved by another program that would not release it".into(),
        ));
    }
    match conn.request_name(name, false, true, true) {
        Ok(RequestNameReply::PrimaryOwner) => Ok(()),
        Ok(_) => Err(busy("another program took the device first".into())),
        Err(e) => Err(busy(e.to_string())),
    }
}
