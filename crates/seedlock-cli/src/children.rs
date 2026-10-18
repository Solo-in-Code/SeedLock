//! The programs that SeedLock starts and keeps running for a while: ffplay for the QR window and for the camera's
//! preview, and ffmpeg for the camera. They are kept here, so that when SeedLock must stop at once (the computer went
//! online, see offline.rs), they end with it at once too, instead of a moment later. (Tested in qrwindow.rs.)

use std::process::Child;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// The programs running now, each with its number.
pub struct Registry {
    running: Mutex<Vec<(u64, Child)>>,
    next: AtomicU64,
}

/// SeedLock's own programs.
static RUNNING: Registry = Registry::new();

impl Registry {
    pub const fn new() -> Registry {
        Registry { running: Mutex::new(Vec::new()), next: AtomicU64::new(1) }
    }

    /// Keeps `child` running until the returned value is dropped, or until [`Registry::end_all`].
    pub fn keep(&'static self, child: Child) -> Running {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut running) = self.running.lock() {
            running.push((id, child));
        }
        Running { registry: self, id }
    }

    /// Ends every program kept here, at once (and waits for each, which takes no time once it was ended).
    pub fn end_all(&self) {
        if let Ok(mut running) = self.running.lock() {
            for (_, child) in running.iter_mut() {
                let _ = child.kill();
            }
            for (_, child) in running.iter_mut() {
                let _ = child.wait();
            }
        }
    }
}

/// A program kept running until this is dropped, when it is ended (and waited for).
pub struct Running {
    registry: &'static Registry,
    id: u64,
}

impl Running {
    /// Keeps `child` running until the returned value is dropped, or until SeedLock stops at once ([`end_all`]).
    pub fn keep(child: Child) -> Running {
        RUNNING.keep(child)
    }

    /// Whether the program has ended by itself (for example, because it could not open its window).
    pub fn ended(&self) -> bool {
        let Ok(mut running) = self.registry.running.lock() else { return true };
        match running.iter_mut().find(|(id, _)| *id == self.id) {
            Some((_, child)) => matches!(child.try_wait(), Ok(Some(_))),
            None => true,
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let child = self
            .registry
            .running
            .lock()
            .ok()
            .and_then(|mut running| running.iter().position(|(id, _)| *id == self.id).map(|at| running.remove(at).1));
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Ends every program SeedLock keeps running, at once (just before SeedLock stops at once, offline.rs).
pub fn end_all() {
    RUNNING.end_all();
}
