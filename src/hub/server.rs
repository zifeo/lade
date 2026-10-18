use std::collections::{HashMap, HashSet};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::peer;
use super::wire::{self, ERR_DENIED, Rep, Req, VersionLedger};

const SECRET_CAP: usize = 256;
const TICKET_CAP: usize = 128;
const TICKET_TTL: Duration = Duration::from_secs(3600);
const IDLE_EXIT: Duration = Duration::from_secs(3600);
// An unreachable hub has nothing left to wait for, so it does not wait out
// `IDLE_EXIT`.
const REACH_CHECK: Duration = Duration::from_secs(60);
const CHECK_SLOT: Duration = Duration::from_secs(3600);
const VERSION_EVERY_MS: i64 = 24 * 3600 * 1000;
// No user waits on this fetch, unlike the 1s one `lade upgrade` used to block on.
const VERSION_FETCH_TIMEOUT: Duration = Duration::from_secs(10);
// Clients give up after 20 ms. One that stalls must not hold every other
// client, and the idle and reach checks, behind it.
const CONN_TIMEOUT: Duration = Duration::from_millis(500);

/// Time since boot, sleep included. `Instant` stops while the machine sleeps
/// (macOS `CLOCK_UPTIME_RAW`, Linux `CLOCK_MONOTONIC`), so a `ttl:` would
/// outlive a closed lid.
fn now() -> Duration {
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

fn since(at: Duration) -> Duration {
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

#[derive(Default)]
pub(crate) struct Tables {
    secrets: HashMap<SecretKey, SecretRow>,
    tickets: HashMap<[u8; 4], TicketRow>,
    windows: HashMap<String, String>,
    version: Arc<Mutex<VersionLedger>>,
    last_check_slot: Option<Duration>,
    version_fetch_due: bool,
}

fn lock(ledger: &Mutex<VersionLedger>) -> std::sync::MutexGuard<'_, VersionLedger> {
    ledger
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Tables {
    fn prune(&mut self) {
        let at = now();
        self.secrets.retain(|_, row| {
            at.saturating_sub(row.inserted) < Duration::from_millis(u64::from(row.ttl_ms))
        });
        self.tickets
            .retain(|_, row| at.saturating_sub(row.inserted) < TICKET_TTL);
    }

    // Windows never expire on their own: they are a `lade cache set`
    // the user asked for, so they keep the hub alive.
    fn is_empty(&mut self) -> bool {
        self.prune();
        self.secrets.is_empty() && self.tickets.is_empty() && self.windows.is_empty()
    }

    fn evict_secrets_if_full(&mut self) {
        while self.secrets.len() > SECRET_CAP {
            let oldest = self
                .secrets
                .iter()
                .min_by_key(|(_, row)| row.inserted)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                self.secrets.remove(&key);
            } else {
                break;
            }
        }
    }

    fn evict_tickets_if_full(&mut self) {
        while self.tickets.len() > TICKET_CAP {
            let oldest = self
                .tickets
                .iter()
                .min_by_key(|(_, row)| row.inserted)
                .map(|(key, _)| *key);
            if let Some(key) = oldest {
                self.tickets.remove(&key);
            } else {
                break;
            }
        }
    }

    fn handle(&mut self, req: Req) -> Rep {
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
                if notice { self.notice() } else { Rep::Ok }
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
                }
            }
            Req::Forget { names } => {
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
                            restamp_scope(&mut self.secrets, &scope, ttl_ms);
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
            Req::Version => Rep::Version(lock(&self.version).clone()),
        }
    }

    fn notice(&mut self) -> Rep {
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

    fn take_version_fetch(&mut self) -> Option<Arc<Mutex<VersionLedger>>> {
        std::mem::take(&mut self.version_fetch_due).then(|| Arc::clone(&self.version))
    }
}

/// A failed fetch keeps the last known `latest`.
fn spawn_version_fetch(ledger: Arc<Mutex<VersionLedger>>) {
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

enum AfterConn {
    Continue,
    Quit,
}

pub async fn serve() -> anyhow::Result<()> {
    serve_until_idle(IDLE_EXIT).await
}

pub(super) async fn serve_until_idle(idle: Duration) -> anyhow::Result<()> {
    let snap = peer::snapshot_self().ok_or_else(|| anyhow::anyhow!("hub image inode"))?;
    let want = snap.inode();
    let path = socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(_) => return Ok(()),
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let bound = peer::inode_of(&path).ok_or_else(|| anyhow::anyhow!("hub socket inode"))?;
    let mut tables = Tables::default();
    let mut last_request = now();
    loop {
        tokio::select! {
            biased;
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                last_request = now();
                let conn = handle_conn(&mut tables, stream, want);
                let after = tokio::time::timeout(CONN_TIMEOUT, conn)
                    .await
                    .unwrap_or(AfterConn::Continue);
                if let Some(ledger) = tables.take_version_fetch() {
                    spawn_version_fetch(ledger);
                }
                if matches!(after, AfterConn::Quit) {
                    break;
                }
            }
            () = tokio::time::sleep(idle.min(REACH_CHECK)) => {
                let reachable =
                    peer::inode_of(&path) == Some(bound) && peer::image_is_current(want);
                if should_exit(&mut tables, reachable, since(last_request) >= idle) {
                    break;
                }
            }
        }
    }
    drop(listener);
    // A client that saw this hub as slow or stale may have unlinked the
    // path and bound a fresh hub there. Never remove that one's socket.
    // The next spawn recreates the lock. Two spawners on two lock files
    // only race the exclusive bind.
    if peer::inode_of(&path) == Some(bound) {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(lock_path());
    }
    Ok(())
}

/// An orphan exits even with live rows: its socket path is gone or belongs
/// to another hub, or a reinstall replaced its binary, so new clients use
/// another socket. Otherwise only an idle hub with empty tables exits.
fn should_exit(tables: &mut Tables, reachable: bool, idle: bool) -> bool {
    !reachable || (idle && tables.is_empty())
}

async fn handle_conn(tables: &mut Tables, mut stream: UnixStream, want: (u64, u64)) -> AfterConn {
    if !peer::peer_allowed(&stream, want) {
        send_denied(&mut stream).await;
        return AfterConn::Continue;
    }
    match read_req(&mut stream).await {
        Some(wire::InReq::Body(req)) => {
            let quit = matches!(req, Req::Quit);
            let Some(frame) = wire::encode_rep(&tables.handle(req)) else {
                return AfterConn::Continue;
            };
            let _ = stream.write_all(&frame).await;
            if quit {
                AfterConn::Quit
            } else {
                AfterConn::Continue
            }
        }
        Some(wire::InReq::WrongVer) => {
            send_denied(&mut stream).await;
            AfterConn::Continue
        }
        None => AfterConn::Continue,
    }
}

async fn send_denied(stream: &mut UnixStream) {
    if let Some(frame) = wire::encode_rep(&Rep::Err(ERR_DENIED)) {
        let _ = stream.write_all(&frame).await;
    }
}

async fn read_req(stream: &mut UnixStream) -> Option<wire::InReq> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).await.ok()?;
    let n = wire::header_len(&header)?;
    let mut body = vec![0u8; n as usize];
    stream.read_exact(&mut body).await.ok()?;
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&body);
    wire::decode_in_req(&frame)
}

pub fn socket_path() -> PathBuf {
    crate::cache::root().join(format!("hub-{}.sock", peer::instance_token()))
}

pub fn lock_path() -> PathBuf {
    crate::cache::root().join(format!("hub-{}.lock", peer::instance_token()))
}

#[cfg(test)]
pub(crate) fn handle_for_test(tables: &mut Tables, req: Req) -> Rep {
    tables.handle(req)
}

#[cfg(test)]
pub(crate) fn empty_tables() -> Tables {
    Tables::default()
}

#[cfg(test)]
pub(crate) fn should_exit_for_test(tables: &mut Tables, reachable: bool, idle: bool) -> bool {
    should_exit(tables, reachable, idle)
}

#[cfg(test)]
pub(crate) fn take_version_fetch_for_test(tables: &mut Tables) -> bool {
    tables.take_version_fetch().is_some()
}

#[cfg(test)]
pub(crate) fn age_ledger_for_test(tables: &mut Tables, by: Duration) {
    let by_ms = i64::try_from(by.as_millis()).expect("test duration");
    let mut ledger = lock(&tables.version);
    ledger.tried_at_ms = ledger.tried_at_ms.map(|at| at - by_ms);
    tables.last_check_slot = tables.last_check_slot.and_then(|at| at.checked_sub(by));
}
