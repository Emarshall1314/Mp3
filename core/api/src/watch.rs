//! Keeps the catalogue current: file-system events (debounced) plus a periodic verification
//! pass for changes events miss (network shares, files edited while the app was closed).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

const DEBOUNCE: Duration = Duration::from_secs(2);
const VERIFY_EVERY: Duration = Duration::from_secs(30 * 60);

pub(crate) struct Watcher {
    _inner: Option<RecommendedWatcher>,
    stop: Arc<AtomicBool>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl Watcher {
    /// Calls `rescan` after changes settle, and every 30 minutes regardless.
    pub(crate) fn start(roots: Vec<PathBuf>, rescan: impl Fn() + Send + 'static) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<()>();
        let mut inner = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res
                && !ev.kind.is_access()
            {
                let _ = tx.send(());
            }
        })
        .ok();
        if let Some(w) = inner.as_mut() {
            for root in &roots {
                // A root that cannot be watched (e.g. inotify limit) still gets periodic scans.
                let _ = w.watch(root, RecursiveMode::Recursive);
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        thread::Builder::new()
            .name("cadenza-watch".into())
            .spawn(move || {
                let mut last_verify = Instant::now();
                let mut dirty_since: Option<Instant> = None;
                while !stopped.load(Ordering::Acquire) {
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(()) => dirty_since = Some(Instant::now()),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => {}
                    }
                    // Fire once events have been quiet for DEBOUNCE (copies of whole albums
                    // produce bursts of events).
                    let settle = dirty_since.is_some_and(|t| t.elapsed() >= DEBOUNCE)
                        && rx.try_recv().is_err();
                    if settle || last_verify.elapsed() >= VERIFY_EVERY {
                        dirty_since = None;
                        last_verify = Instant::now();
                        if !roots.is_empty() {
                            rescan();
                        }
                    }
                }
            })
            .ok()?;
        Some(Self {
            _inner: inner,
            stop,
        })
    }
}
