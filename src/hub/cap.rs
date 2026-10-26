//! `max_cache` from `config.json`, watched by the hub. A thread does the
//! `stat` and the read, so the request loop never touches the file: it only
//! reads the last value under a lock no IO ever holds.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const WATCH_EVERY: Duration = Duration::from_secs(1);

/// mtime, size, inode. An atomic rename gets a new inode.
type Stamp = (SystemTime, u64, u64);

/// A tunnel or secret ttl under `max_cache`. `None` caps nothing, `Some(0)`
/// keeps nothing.
pub(super) fn capped(ttl_ms: Option<u32>, cap_ms: Option<u32>) -> Option<u32> {
    match cap_ms {
        None => ttl_ms,
        Some(0) => None,
        Some(cap) => ttl_ms.map(|ttl| ttl.min(cap)),
    }
}

/// What the watcher last read. `generation` moves on every value change.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Seen {
    generation: u64,
    cap_ms: Option<u32>,
}

struct Watch {
    path: PathBuf,
    seen: Mutex<Seen>,
    /// `None` after a failed read, so the next pass retries.
    stamp: Mutex<Option<Stamp>>,
    stop: AtomicBool,
}

impl Watch {
    /// One pass: reads the file only when its stamp moved, or after a read
    /// failed. Returns whether the value changed.
    fn check(&self) -> bool {
        let stamp = stamp(&self.path);
        let mut last = self.stamp.lock().unwrap_or_else(|e| e.into_inner());
        if stamp.is_some() && stamp == *last {
            return false;
        }
        let (cap_ms, readable) = read_cap(&self.path);
        *last = if readable { stamp } else { None };
        drop(last);
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.cap_ms == cap_ms {
            return false;
        }
        *seen = Seen {
            generation: seen.generation + 1,
            cap_ms,
        };
        true
    }
}

pub(super) struct CapWatch {
    watch: Arc<Watch>,
    /// The generation the tables last applied.
    applied: u64,
}

impl CapWatch {
    /// Reads the file once before the hub serves, then watches it.
    pub(super) fn spawn(path: PathBuf) -> Self {
        let watch = Arc::new(Watch {
            path,
            seen: Mutex::new(Seen {
                generation: 0,
                cap_ms: None,
            }),
            stamp: Mutex::new(None),
            stop: AtomicBool::new(false),
        });
        watch.check();
        let applied = watch
            .seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .generation;
        let background = Arc::clone(&watch);
        std::thread::spawn(move || {
            while !background.stop.load(Ordering::Relaxed) {
                std::thread::sleep(WATCH_EVERY);
                background.check();
            }
        });
        Self { watch, applied }
    }

    pub(super) fn cap_ms(&self) -> Option<u32> {
        self.watch
            .seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cap_ms
    }

    /// `Some(cap)` once per value change the tables have not applied yet.
    pub(super) fn take_change(&mut self) -> Option<Option<u32>> {
        let seen = *self.watch.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.generation == self.applied {
            return None;
        }
        self.applied = seen.generation;
        Some(seen.cap_ms)
    }

    /// Tests run the watcher's pass now instead of waiting for the thread.
    #[cfg(test)]
    pub(super) fn check_now(&self) {
        self.watch.check();
    }
}

impl Drop for CapWatch {
    fn drop(&mut self) {
        self.watch.stop.store(true, Ordering::Relaxed);
    }
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len(), meta.ino()))
}

/// Fails closed: a file the hub cannot read or parse turns the cache off.
/// The flag is false when the read itself failed, so the watcher retries.
fn read_cap(path: &Path) -> (Option<u32>, bool) {
    match std::fs::read_to_string(path) {
        Ok(text) => (
            crate::global_config::max_cache_from_text(&text).unwrap_or(Some(0)),
            true,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, true),
        Err(_) => (Some(0), false),
    }
}
