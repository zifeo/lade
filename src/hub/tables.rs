use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use super::apply::EVAL_WHEN;
use super::cap::{CapWatch, capped};
use super::links::Links;
use super::release::ReleaseCheck;
use super::tunnels::Tunnels;
use super::wire::{self, Rep, Req};

const SECRET_CAP: usize = 256;
const TICKET_CAP: usize = 128;
const TICKET_TTL: Duration = Duration::from_secs(3600);

/// Time since boot, sleep included. `Instant` stops while the machine sleeps
/// (macOS `CLOCK_UPTIME_RAW`, Linux `CLOCK_MONOTONIC`), so a `ttl:` would
/// outlive a closed lid.
pub(super) fn now() -> Duration {
    // § hadg0brg4.b@ze5gc
    #[cfg(target_os = "linux")]
    const CLOCK: libc::clockid_t = libc::CLOCK_BOOTTIME;
    #[cfg(not(target_os = "linux"))]
    const CLOCK: libc::clockid_t = libc::CLOCK_MONOTONIC;
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_gettime(CLOCK, &mut ts) };
    assert_eq!(rc, 0, "clock_gettime");
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32) + clock_ahead()
}

#[cfg(not(test))]
fn clock_ahead() -> Duration {
    Duration::ZERO
}

#[cfg(test)]
fn clock_ahead() -> Duration {
    CLOCK_AHEAD.with(std::cell::Cell::get)
}

pub(super) fn since(at: Duration) -> Duration {
    now().saturating_sub(at)
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct SecretKey {
    // § je3g2tk0d.a@m9gs6
    cwd: String,
    path: String,
    rule: String,
    when: String,
    walk: [u8; 32],
    user: String,
}

struct SecretRow {
    inserted: Duration,
    ttl_ms: u32,
    bindings: Vec<(String, Vec<u8>)>,
}

/// A row leaves no value behind in freed memory.
impl Drop for SecretRow {
    fn drop(&mut self) {
        wipe(self);
    }
}

fn wipe(row: &mut SecretRow) {
    // § d3r0zsk5r.a@8ymbd
    for (_, value) in &mut row.bindings {
        zeroize::Zeroize::zeroize(value);
    }
}

/// Takes the `drop` names out of `row`, each value zeroed before it goes.
/// Returns the removed buffers, already zeroed.
fn forget_in_row(row: &mut SecretRow, drop: &HashSet<String>) -> Vec<Vec<u8>> {
    // § d3r0zsk5r.b@7nm7v
    let (removed, kept) = std::mem::take(&mut row.bindings)
        .into_iter()
        .partition::<Vec<_>, _>(|(name, _)| drop.contains(name));
    row.bindings = kept;
    removed
        .into_iter()
        .map(|(_, mut value)| {
            zeroize::Zeroize::zeroize(&mut value);
            value
        })
        .collect()
}

struct TicketRow {
    inserted: Duration,
    ticket: Vec<u8>,
}

pub(crate) struct Tables {
    secrets: HashMap<SecretKey, SecretRow>,
    tickets: HashMap<[u8; 4], TicketRow>,
    windows: HashMap<String, String>,
    pub(super) release: ReleaseCheck,
    pub(super) tunnels: Tunnels,
    links: Links,
    cap: CapWatch,
    /// Why this hub caches no secret: another process of the user may read
    /// its memory. Tickets and tunnels hold no secret value.
    unguarded: Option<String>,
}

impl Tables {
    /// `config` is the `config.json` whose `max_cache` caps every ttl.
    pub(super) fn new(config: PathBuf, unguarded: Option<String>) -> Self {
        let cap = CapWatch::spawn(config);
        let mut tunnels = Tunnels::default();
        tunnels.set_cap(cap.cap_ms());
        Self {
            secrets: HashMap::new(),
            tickets: HashMap::new(),
            windows: HashMap::new(),
            release: ReleaseCheck::default(),
            tunnels,
            links: Links::default(),
            cap,
            unguarded,
        }
    }

    /// Any `max_cache` change, up or down, drops every cached secret and
    /// every tunnel no command holds: a raised cap never extends old rows.
    fn refresh_cap(&mut self) {
        // § cfan04ksd.b@6kf57
        if let Some(cap_ms) = self.cap.take_change() {
            self.secrets.clear();
            self.tunnels.set_cap(cap_ms);
        }
    }

    fn prune(&mut self) {
        self.refresh_cap();
        let at = now();
        self.secrets.retain(|_, row| {
            at.saturating_sub(row.inserted) < Duration::from_millis(u64::from(row.ttl_ms))
        });
        // § one spzvjnk1e.b@ahycb
        self.tickets
            .retain(|_, row| at.saturating_sub(row.inserted) < TICKET_TTL);
        self.tunnels.tick(at);
        self.links.prune();
    }

    // Windows never expire on their own: they are a `lade ttl set`
    // the user asked for, so they keep the hub alive.
    pub(super) fn is_empty(&mut self) -> bool {
        // § cbz2he75h.a@01bee
        self.prune();
        self.secrets.is_empty()
            && self.tickets.is_empty()
            && self.windows.is_empty()
            && self.tunnels.is_empty()
            && self.links.is_empty()
    }

    fn evict_secrets_if_full(&mut self) {
        // § gn0zrhfpq@mx133
        evict_oldest(&mut self.secrets, SECRET_CAP, |row: &SecretRow| {
            row.inserted
        });
    }

    fn evict_tickets_if_full(&mut self) {
        // § spzvjnk1e.b@tdcta
        evict_oldest(&mut self.tickets, TICKET_CAP, |row: &TicketRow| {
            row.inserted
        });
    }

    pub(super) fn handle(&mut self, req: Req) -> Rep {
        // § one hadg0brg4.c@k7aeh
        self.prune();
        match req {
            Req::Get {
                cwd,
                path,
                rule,
                when,
                walk,
                user,
            } => {
                // § hadg0brg4.a@qy78r
                let key = SecretKey {
                    cwd,
                    path,
                    rule,
                    when,
                    walk,
                    user,
                };
                match self.secrets.get(&key) {
                    Some(row) => Rep::Hit {
                        bindings: row.bindings.clone(),
                    },
                    None => Rep::Miss,
                }
            }
            Req::Put {
                cwd,
                path,
                rule,
                when,
                walk,
                user,
                ttl_ms,
                bindings,
            } => {
                if self.unguarded.is_some() {
                    // § kgejwr97c.a@1k5hk
                    return Rep::Ok;
                }
                // § one cfan04ksd.a@mrfx5
                let Some(ttl_ms) = capped(Some(ttl_ms), self.cap.cap_ms()) else {
                    return Rep::Ok;
                };
                let key = SecretKey {
                    cwd,
                    path,
                    rule,
                    when,
                    walk,
                    user,
                };
                self.secrets.insert(
                    key,
                    SecretRow {
                        inserted: now(),
                        ttl_ms,
                        bindings,
                    },
                );
                self.evict_secrets_if_full();
                Rep::Ok
            }
            Req::GetT { t } => match self.tickets.get(&t) {
                Some(row) => Rep::Ticket {
                    ticket: row.ticket.clone(),
                },
                None => Rep::Miss,
            },
            Req::PutT { t, ticket, notice } => {
                self.tickets.insert(
                    t,
                    TicketRow {
                        inserted: now(),
                        ticket,
                    },
                );
                self.evict_tickets_if_full();
                if notice {
                    self.release.notice()
                } else {
                    Rep::Ok
                }
            }
            Req::UnlinkT { t } => {
                // § spzvjnk1e.b@hgjg7
                self.tickets.remove(&t);
                Rep::Ok
            }
            Req::Notice => self.release.notice(),
            Req::Ping => Rep::Stat {
                pid: std::process::id(),
                secrets: self.secrets.len() as u32,
                tickets: self.tickets.len() as u32,
                unguarded: self.unguarded.clone(),
            },
            Req::Quit => Rep::Ok,
            Req::List => {
                // § f1t0s88fs.a@gqmjh
                let mut rows = Vec::new();
                for (key, row) in &self.secrets {
                    let left = ttl_left_ms(row);
                    for (name, _) in &row.bindings {
                        rows.push(wire::CacheRow {
                            name: name.clone(),
                            cwd: key.cwd.clone(),
                            path: key.path.clone(),
                            rule: key.rule.clone(),
                            when: key.when.clone(),
                            user: key.user.clone(),
                            ttl_left_ms: left,
                        });
                    }
                }
                rows.sort_by(|a, b| {
                    a.name
                        .cmp(&b.name)
                        .then(a.cwd.cmp(&b.cwd))
                        .then(a.rule.cmp(&b.rule))
                });
                Rep::Listing {
                    pid: std::process::id(),
                    tickets: self.tickets.len() as u32,
                    rows,
                    tunnels: self.tunnels.rows(now()),
                    max_cache_ms: self.cap.cap_ms(),
                }
            }
            Req::Forget { names } => {
                // § wszszmyqk.b@xdt3k
                self.tunnels.forget(&names);
                let drop: HashSet<String> = names.into_iter().collect();
                // § one d3r0zsk5r.b@64a09
                for row in self.secrets.values_mut() {
                    forget_in_row(row, &drop);
                }
                self.secrets.retain(|_, row| !row.bindings.is_empty());
                Rep::Ok
            }
            Req::SetWindow { scope, ttl } => {
                match ttl {
                    Some(raw) => {
                        let parsed = crate::config::RuleTtl::parse(&raw);
                        self.windows.insert(scope.clone(), raw);
                        if let Ok(crate::config::RuleTtl::Window(duration)) = parsed {
                            let ttl_ms = crate::config::duration_to_ttl_ms(duration);
                            // § cfan04ksd.a@5ez7x
                            match capped(Some(ttl_ms), self.cap.cap_ms()) {
                                Some(ttl_ms) => {
                                    restamp_scope(&mut self.secrets, &self.windows, &scope, ttl_ms)
                                }
                                None => self.secrets.clear(),
                            }
                        }
                    }
                    None => {
                        // § zdyc7ggsy.d@8avjz
                        self.windows.remove(&scope);
                    }
                }
                Rep::Ok
            }
            Req::GetWindow { scope } => Rep::Window {
                ttl: window_for(&self.windows, &scope).map(|(_, ttl)| ttl.clone()),
            },
            Req::Version => Rep::Version(self.release.ledger()),
            Req::TunnelOpen(ask) => Rep::Tunnel(self.tunnels.open(ask)),
            Req::TunnelPoll { id } => Rep::Tunnel(self.tunnels.poll(id)),
            Req::TunnelRelease { lease } => {
                self.tunnels.release(&lease, now());
                Rep::Ok
            }
            Req::LinkStart {
                key,
                host,
                spec,
                env,
                walk,
            } => Rep::Links(vec![self.links.start(key, host, spec, env, walk)]),
            Req::LinkPoll { key } => Rep::Links(self.links.poll(&key)),
            Req::LinkStop { key } => {
                if self.links.stop(&key) {
                    Rep::Ok
                } else {
                    Rep::Links(Vec::new())
                }
            }
            Req::LinkList => Rep::Links(self.links.list()),
        }
    }
}

fn restamp_scope(
    secrets: &mut HashMap<SecretKey, SecretRow>,
    windows: &HashMap<String, String>,
    scope: &str,
    ttl_ms: u32,
) {
    let at = now();
    // § one zdyc7ggsy.c@n3rqs, y8w7xhrr1.d@qrsat
    for (key, row) in secrets.iter_mut() {
        let governs = window_for(windows, &key.cwd).is_some_and(|(nearest, _)| nearest == scope);
        if governs && key.when != EVAL_WHEN {
            row.inserted = at;
            row.ttl_ms = ttl_ms;
        }
    }
}

fn window_for<'a>(
    windows: &'a HashMap<String, String>,
    cwd: &str,
) -> Option<(&'a String, &'a String)> {
    // § zdyc7ggsy.b@gwprf
    windows
        .iter()
        .filter(|(scope, _)| path_related(cwd, scope))
        .min_by_key(|(scope, _)| {
            (
                depth(scope).abs_diff(depth(cwd)),
                is_path_prefix(scope, cwd),
                scope.as_str(),
            )
        })
}

fn depth(path: &str) -> usize {
    path.split('/').filter(|part| !part.is_empty()).count()
}

pub(crate) fn path_related(cwd: &str, scope: &str) -> bool {
    cwd == scope || is_path_prefix(cwd, scope) || is_path_prefix(scope, cwd)
}

fn is_path_prefix(child: &str, parent: &str) -> bool {
    // `/` is the one parent that already ends with the separator.
    let parent = parent.strip_suffix('/').unwrap_or(parent);
    child.len() > parent.len() + 1
        && child.starts_with(parent)
        && child.as_bytes()[parent.len()] == b'/'
}

fn ttl_left_ms(row: &SecretRow) -> u32 {
    let elapsed = u32::try_from(since(row.inserted).as_millis()).unwrap_or(u32::MAX);
    row.ttl_ms.saturating_sub(elapsed)
}

fn evict_oldest<K, V>(map: &mut HashMap<K, V>, cap: usize, inserted: impl Fn(&V) -> Duration)
where
    K: std::hash::Hash + Eq + Clone,
{
    while map.len() > cap {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, row)| inserted(row))
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        map.remove(&oldest);
    }
}

#[cfg(test)]
pub(crate) fn handle_for_test(tables: &mut Tables, req: Req) -> Rep {
    tables.handle(req)
}

#[cfg(test)]
pub(crate) fn empty_tables() -> Tables {
    // The temp dir goes at once: no `config.json`, so no cap.
    Tables::new(
        tempfile::tempdir().unwrap().path().join("config.json"),
        None,
    )
}

#[cfg(test)]
pub(crate) fn unguarded_tables() -> Tables {
    Tables::new(
        tempfile::tempdir().unwrap().path().join("config.json"),
        Some("the hub could not make itself not dumpable".to_string()),
    )
}

#[cfg(test)]
pub(crate) fn tables_with_config(config: PathBuf) -> Tables {
    Tables::new(config, None)
}

#[cfg(test)]
pub(crate) fn check_cap_for_test(tables: &Tables) {
    tables.cap.check_now();
}

#[cfg(test)]
pub(crate) fn take_version_fetch_for_test(tables: &mut Tables) -> bool {
    tables.release.take_version_fetch().is_some()
}

/// Moves this thread's hub clock forward. Moving a row back instead fails
/// on a machine booted less than `by` ago, such as a fresh CI runner.
#[cfg(test)]
pub(crate) fn advance_clock_for_test(by: Duration) {
    CLOCK_AHEAD.with(|ahead| ahead.set(ahead.get() + by));
}

#[cfg(test)]
thread_local! {
    static CLOCK_AHEAD: std::cell::Cell<Duration> = const { std::cell::Cell::new(Duration::ZERO) };
}

#[cfg(test)]
pub(crate) fn age_ledger_for_test(tables: &mut Tables, by: Duration) {
    tables.release.age(by);
}

#[cfg(test)]
mod row_tests {
    use super::*;

    fn row(bindings: &[(&str, &[u8])]) -> SecretRow {
        SecretRow {
            inserted: now(),
            ttl_ms: 60_000,
            bindings: bindings
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_vec()))
                .collect(),
        }
    }

    /// `Vec` zeroize clears the length and zeroes the whole capacity, so
    /// the bytes are read back up to the capacity.
    fn every_byte_is_zero(mut value: Vec<u8>) -> bool {
        assert!(value.is_empty());
        let capacity = value.capacity();
        assert!(capacity > 0);
        // Safety: zeroize wrote every byte up to the capacity.
        unsafe { value.set_len(capacity) };
        value.iter().all(|byte| *byte == 0)
    }

    #[test]
    fn forget_zeroes_the_removed_values_and_keeps_the_rest() {
        let mut secrets = row(&[("AWS", b"aws-secret"), ("DB", b"db-secret")]);
        let drop: HashSet<String> = ["AWS".to_string()].into();
        let removed = forget_in_row(&mut secrets, &drop);
        assert_eq!(removed.len(), 1);
        assert!(removed.into_iter().all(every_byte_is_zero));
        assert_eq!(
            secrets.bindings,
            vec![("DB".to_string(), b"db-secret".to_vec())]
        );
    }

    #[test]
    fn wipe_zeroes_every_value() {
        let mut secrets = row(&[("AWS", b"aws-secret"), ("DB", b"db-secret")]);
        wipe(&mut secrets);
        let values: Vec<Vec<u8>> = std::mem::take(&mut secrets.bindings)
            .into_iter()
            .map(|(_, value)| value)
            .collect();
        assert_eq!(values.len(), 2);
        assert!(values.into_iter().all(every_byte_is_zero));
    }

    /// Sleep itself cannot run in CI: this pins the clock the hub reads.
    #[test]
    fn now_counts_sleep() {
        #[cfg(target_os = "linux")]
        const SLEEP_COUNTING: libc::clockid_t = libc::CLOCK_BOOTTIME;
        #[cfg(not(target_os = "linux"))]
        const SLEEP_COUNTING: libc::clockid_t = libc::CLOCK_MONOTONIC;
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let before = now();
        assert_eq!(unsafe { libc::clock_gettime(SLEEP_COUNTING, &mut ts) }, 0);
        let after = now();
        let reference = Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32);
        assert!(before <= reference && reference <= after);
        assert!(after - before < Duration::from_millis(5));
    }
}
