use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::tables::{now, since};
use super::wire::{Rep, VersionLedger};

const CHECK_SLOT: Duration = Duration::from_secs(3600);
const VERSION_EVERY_MS: i64 = 24 * 3600 * 1000;
// No user waits on this fetch, unlike the 1s one `lade upgrade` used to block on.
const VERSION_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// The latest release, fetched at most daily, and the hourly slot for shell
/// notices.
#[derive(Default)]
pub(super) struct ReleaseCheck {
    version: Arc<Mutex<VersionLedger>>,
    last_check_slot: Option<Duration>,
    version_fetch_due: bool,
}

fn lock(ledger: &Mutex<VersionLedger>) -> std::sync::MutexGuard<'_, VersionLedger> {
    ledger
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl ReleaseCheck {
    pub(super) fn ledger(&self) -> VersionLedger {
        lock(&self.version).clone()
    }

    pub(super) fn notice(&mut self) -> Rep {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let version = {
            let mut ledger = lock(&self.version);
            if ledger
                .tried_at_ms
                .is_none_or(|tried| now_ms - tried >= VERSION_EVERY_MS)
            {
                // Stamp before the fetch so concurrent shells start one.
                ledger.tried_at_ms = Some(now_ms);
                self.version_fetch_due = true;
            }
            ledger.clone()
        };
        let check = self
            .last_check_slot
            .is_none_or(|at| since(at) >= CHECK_SLOT);
        if check {
            self.last_check_slot = Some(now());
        }
        Rep::Notice { check, version }
    }

    pub(super) fn take_version_fetch(&mut self) -> Option<Arc<Mutex<VersionLedger>>> {
        std::mem::take(&mut self.version_fetch_due).then(|| Arc::clone(&self.version))
    }

    #[cfg(test)]
    pub(super) fn age(&mut self, by: Duration) {
        let by_ms = i64::try_from(by.as_millis()).expect("test duration");
        let mut ledger = lock(&self.version);
        ledger.tried_at_ms = ledger.tried_at_ms.map(|at| at - by_ms);
        self.last_check_slot = self.last_check_slot.and_then(|at| at.checked_sub(by));
    }
}

/// A failed fetch keeps the last known `latest`.
pub(super) fn spawn_version_fetch(ledger: Arc<Mutex<VersionLedger>>) {
    tokio::task::spawn_blocking(move || {
        let result = crate::command::upgrade::fetch_latest_tag(VERSION_FETCH_TIMEOUT);
        let mut ledger = lock(&ledger);
        match result {
            Ok(tag) => {
                ledger.latest = Some(tag);
                ledger.error = None;
            }
            Err(error) => ledger.error = Some(format!("{error:#}")),
        }
    });
}
