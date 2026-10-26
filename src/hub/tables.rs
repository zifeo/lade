use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use super::cap::{CapWatch, capped};
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
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

pub(super) fn since(at: Duration) -> Duration {
    now().saturating_sub(at)
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct SecretKey {
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
    cap: CapWatch,
}

impl Tables {
    /// `config` is the `config.json` whose `max_cache` caps every ttl.
    pub(super) fn new(config: PathBuf) -> Self {
        let cap = CapWatch::spawn(config);
        let mut tunnels = Tunnels::default();
        tunnels.set_cap(cap.cap_ms());
        Self {
            secrets: HashMap::new(),
            tickets: HashMap::new(),
            windows: HashMap::new(),
            release: ReleaseCheck::default(),
            tunnels,
            cap,
        }
    }

    /// Any `max_cache` change, up or down, drops every cached secret and
    /// every tunnel no command holds: a raised cap never extends old rows.
    fn refresh_cap(&mut self) {
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
        self.tickets
            .retain(|_, row| at.saturating_sub(row.inserted) < TICKET_TTL);
        self.tunnels.tick(at);
    }

    // Windows never expire on their own: they are a `lade ttl set`
    // the user asked for, so they keep the hub alive.
    pub(super) fn is_empty(&mut self) -> bool {
        self.prune();
        self.secrets.is_empty()
            && self.tickets.is_empty()
            && self.windows.is_empty()
            && self.tunnels.is_empty()
    }

    fn evict_secrets_if_full(&mut self) {
        evict_oldest(&mut self.secrets, SECRET_CAP, |row: &SecretRow| {
            row.inserted
        });
    }

    fn evict_tickets_if_full(&mut self) {
        evict_oldest(&mut self.tickets, TICKET_CAP, |row: &TicketRow| {
            row.inserted
        });
    }

    pub(super) fn handle(&mut self, req: Req) -> Rep {
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
                self.tickets.remove(&t);
                Rep::Ok
            }
            Req::Ping => Rep::Stat {
                pid: std::process::id(),
                secrets: self.secrets.len() as u32,
                tickets: self.tickets.len() as u32,
            },
            Req::Quit => Rep::Ok,
            Req::List => {
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
                self.tunnels.forget(&names);
                let drop: HashSet<String> = names.into_iter().collect();
                for row in self.secrets.values_mut() {
                    row.bindings.retain(|(name, _)| !drop.contains(name));
                }
                self.secrets.retain(|_, row| !row.bindings.is_empty());
                Rep::Ok
            }
            Req::SetWindow { scope, ttl } => {
                match ttl {
                    Some(raw) => {
                        if let Ok(crate::config::RuleTtl::Window(duration)) =
                            crate::config::RuleTtl::parse(&raw)
                        {
                            let ttl_ms = crate::config::duration_to_ttl_ms(duration);
                            match capped(Some(ttl_ms), self.cap.cap_ms()) {
                                Some(ttl_ms) => restamp_scope(&mut self.secrets, &scope, ttl_ms),
                                None => self.secrets.clear(),
                            }
                        }
                        self.windows.insert(scope, raw);
                    }
                    None => {
                        self.windows.remove(&scope);
                    }
                }
                Rep::Ok
            }
            Req::GetWindow { scope } => Rep::Window {
                ttl: window_for(&self.windows, &scope),
            },
            Req::Version => Rep::Version(self.release.ledger()),
            Req::TunnelOpen(ask) => Rep::Tunnel(self.tunnels.open(ask)),
            Req::TunnelPoll { id } => Rep::Tunnel(self.tunnels.poll(id)),
            Req::TunnelRelease { lease } => {
                self.tunnels.release(&lease, now());
                Rep::Ok
            }
        }
    }
}

fn restamp_scope(secrets: &mut HashMap<SecretKey, SecretRow>, scope: &str, ttl_ms: u32) {
    let at = now();
    for (key, row) in secrets.iter_mut() {
        if path_related(&key.cwd, scope) {
            row.inserted = at;
            row.ttl_ms = ttl_ms;
        }
    }
}

fn window_for(windows: &HashMap<String, String>, cwd: &str) -> Option<String> {
    windows
        .iter()
        .filter(|(scope, _)| path_related(cwd, scope))
        .max_by_key(|(scope, _)| scope.len())
        .map(|(_, ttl)| ttl.clone())
}

pub(crate) fn path_related(cwd: &str, scope: &str) -> bool {
    if cwd == "/" || scope == "/" {
        return cwd == scope;
    }
    cwd == scope || is_path_prefix(cwd, scope) || is_path_prefix(scope, cwd)
}

fn is_path_prefix(child: &str, parent: &str) -> bool {
    child.starts_with(parent) && child.as_bytes().get(parent.len()) == Some(&b'/')
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
    Tables::new(tempfile::tempdir().unwrap().path().join("config.json"))
}

#[cfg(test)]
pub(crate) fn tables_with_config(config: PathBuf) -> Tables {
    Tables::new(config)
}

#[cfg(test)]
pub(crate) fn check_cap_for_test(tables: &Tables) {
    tables.cap.check_now();
}

#[cfg(test)]
pub(crate) fn take_version_fetch_for_test(tables: &mut Tables) -> bool {
    tables.release.take_version_fetch().is_some()
}

#[cfg(test)]
pub(crate) fn age_ledger_for_test(tables: &mut Tables, by: Duration) {
    tables.release.age(by);
}
