use serde::Serialize;

use super::server::socket_path;
use super::transport::{COMMAND_DEADLINE, rpc_existing, rpc_within};
use super::wire::{
    CacheRow, ERR_DENIED, LinkRow, Rep, Req, TunnelAsk, TunnelRep, TunnelRow, VersionLedger,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HubState {
    Down,
    Up,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HubInfo {
    pub state: HubState,
    pub pid: Option<u32>,
    pub secrets: u32,
    pub tickets: u32,
    /// Why a hub that is up caches no secret. `memory` in status says it.
    #[serde(skip)]
    pub unguarded: Option<String>,
}

impl HubInfo {
    fn down() -> Self {
        Self {
            state: HubState::Down,
            pid: None,
            secrets: 0,
            tickets: 0,
            unguarded: None,
        }
    }

    fn stale() -> Self {
        Self {
            state: HubState::Stale,
            pid: None,
            secrets: 0,
            tickets: 0,
            unguarded: None,
        }
    }

    fn up(pid: u32, secrets: u32, tickets: u32, unguarded: Option<String>) -> Self {
        Self {
            state: HubState::Up,
            pid: Some(pid),
            secrets,
            tickets,
            unguarded,
        }
    }
}

pub fn probe() -> HubInfo {
    // § f18xcggkd.c@j5w9a
    match rpc_existing(Req::Ping) {
        Some(Rep::Stat {
            pid,
            secrets,
            tickets,
            unguarded,
        }) => HubInfo::up(pid, secrets, tickets, unguarded),
        Some(Rep::Err(code)) if code == ERR_DENIED => HubInfo::stale(),
        _ => HubInfo::down(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubStop {
    Down,
    Stale,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheListing {
    pub state: HubState,
    pub pid: Option<u32>,
    pub tickets: u32,
    pub rows: Vec<CacheRow>,
    pub tunnels: Vec<TunnelRow>,
    pub max_cache_ms: Option<u32>,
}

impl CacheListing {
    fn down() -> Self {
        Self {
            state: HubState::Down,
            pid: None,
            tickets: 0,
            rows: Vec::new(),
            tunnels: Vec::new(),
            max_cache_ms: None,
        }
    }

    fn stale() -> Self {
        Self {
            state: HubState::Stale,
            pid: None,
            tickets: 0,
            rows: Vec::new(),
            tunnels: Vec::new(),
            max_cache_ms: None,
        }
    }

    fn up(
        pid: u32,
        tickets: u32,
        rows: Vec<CacheRow>,
        tunnels: Vec<TunnelRow>,
        max_cache_ms: Option<u32>,
    ) -> Self {
        Self {
            state: HubState::Up,
            pid: Some(pid),
            tickets,
            rows,
            tunnels,
            max_cache_ms,
        }
    }
}

pub fn listing() -> CacheListing {
    // § f18xcggkd.c@qx9fd
    match rpc_existing(Req::List) {
        Some(Rep::Listing {
            pid,
            tickets,
            rows,
            tunnels,
            max_cache_ms,
        }) => CacheListing::up(pid, tickets, rows, tunnels, max_cache_ms),
        Some(Rep::Err(code)) if code == ERR_DENIED => CacheListing::stale(),
        _ => CacheListing::down(),
    }
}

pub fn scope(cwd: &std::path::Path) -> String {
    cwd.to_string_lossy().into_owned()
}

pub fn set_window(scope: &str, ttl: &str) -> Option<()> {
    match rpc_within(
        Req::SetWindow {
            scope: scope.to_string(),
            ttl: Some(ttl.to_string()),
        },
        true,
        COMMAND_DEADLINE,
    ) {
        Some(Rep::Ok) => Some(()),
        _ => None,
    }
}

pub fn unset_window(scope: &str) -> Option<()> {
    match rpc_within(
        Req::SetWindow {
            scope: scope.to_string(),
            ttl: None,
        },
        true,
        COMMAND_DEADLINE,
    ) {
        Some(Rep::Ok) => Some(()),
        _ => None,
    }
}

pub fn window(scope: &str) -> Option<String> {
    // § f18xcggkd.c@k3g7z
    match rpc_existing(Req::GetWindow {
        scope: scope.to_string(),
    }) {
        Some(Rep::Window { ttl }) => ttl,
        _ => None,
    }
}

pub fn forget_names(names: &[String]) -> Option<()> {
    if names.is_empty() {
        return None;
    }
    // § f18xcggkd.c@st89g
    match rpc_within(
        Req::Forget {
            names: names.to_vec(),
        },
        false,
        COMMAND_DEADLINE,
    ) {
        Some(Rep::Ok) => Some(()),
        _ => None,
    }
}

pub fn stop() -> HubStop {
    // § f18xcggkd.c@x8tpg
    let result = match rpc_existing(Req::Quit) {
        Some(Rep::Ok) => HubStop::Stopped,
        Some(Rep::Err(code)) if code == ERR_DENIED => HubStop::Stale,
        _ => HubStop::Down,
    };
    // A timeout is not Quit. Unlinking that socket orphans a live hub.
    if matches!(result, HubStop::Stopped | HubStop::Stale)
        && let Some(sock) = socket_path()
    {
        let _ = std::fs::remove_file(sock);
    }
    result
}

/// Never starts a hub. `None`: off, down, or stale.
pub fn version_ledger() -> Option<VersionLedger> {
    // § f18xcggkd.c@kxqzf
    match rpc_existing(Req::Version) {
        Some(Rep::Version(version)) => Some(version),
        _ => None,
    }
}

pub fn put_t(id: &str) {
    // § spzvjnk1e.a@mkz1y
    if let Some(req) = put_t_req(id, false) {
        let _ = rpc_existing(req);
    }
}

/// `put_t` that also asks for the hourly notice slot. Starts the hub when it
/// is down. `Some` only when this caller owns the slot.
/// The hourly slot for a pre-tool wrap, which already uses the hub.
pub fn notice_slot() -> Option<VersionLedger> {
    match rpc_within(Req::Notice, true, COMMAND_DEADLINE) {
        Some(Rep::Notice {
            check: true,
            version,
        }) => Some(version),
        _ => None,
    }
}

pub fn put_t_notice(id: &str) -> Option<VersionLedger> {
    match rpc_within(put_t_req(id, true)?, true, COMMAND_DEADLINE) {
        Some(Rep::Notice {
            check: true,
            version,
        }) => Some(version),
        _ => None,
    }
}

fn put_t_req(id: &str, notice: bool) -> Option<Req> {
    if !crate::ticket::is_id(id) {
        return None;
    }
    let ticket = std::fs::read(crate::ticket::path(id)).ok()?;
    let mut t = [0u8; 4];
    t.copy_from_slice(id.as_bytes());
    Some(Req::PutT { t, ticket, notice })
}

pub fn unlink_t(id: &str) {
    if !crate::ticket::is_id(id) {
        return;
    }
    let mut t = [0u8; 4];
    t.copy_from_slice(id.as_bytes());
    // § one f18xcggkd.b@h0p7q, spzvjnk1e.a@e8s8p
    let _ = rpc_existing(Req::UnlinkT { t });
}

/// Starts the hub when it is down. `None`: off, down, or stale.
pub fn tunnel_open(ask: TunnelAsk) -> Option<TunnelRep> {
    match rpc_within(Req::TunnelOpen(ask), true, COMMAND_DEADLINE) {
        Some(Rep::Tunnel(rep)) => Some(rep),
        _ => None,
    }
}

pub fn tunnel_poll(id: u64) -> Option<TunnelRep> {
    match rpc_within(Req::TunnelPoll { id }, false, COMMAND_DEADLINE) {
        Some(Rep::Tunnel(rep)) => Some(rep),
        _ => None,
    }
}

/// Never starts a hub: a hub that is down holds no tunnel.
pub fn tunnel_release(lease: &str) {
    // § f18xcggkd.b@m803q
    let _ = rpc_within(
        Req::TunnelRelease {
            lease: lease.to_string(),
        },
        false,
        COMMAND_DEADLINE,
    );
}

/// Starts the hub when it is down: it holds the link from here on.
pub fn link_start(
    key: String,
    host: String,
    spec: String,
    env: Vec<(Vec<u8>, Vec<u8>)>,
    walk: Option<(String, String)>,
) -> Option<Vec<LinkRow>> {
    match rpc_within(
        Req::LinkStart {
            key,
            host,
            spec,
            env,
            walk,
        },
        true,
        COMMAND_DEADLINE,
    ) {
        Some(Rep::Links(rows)) => Some(rows),
        _ => None,
    }
}

pub fn link_poll(key: &str) -> Option<Vec<LinkRow>> {
    match rpc_within(
        Req::LinkPoll {
            key: key.to_string(),
        },
        false,
        COMMAND_DEADLINE,
    ) {
        Some(Rep::Links(rows)) => Some(rows),
        _ => None,
    }
}

/// `Some(false)`: no link on `key`. `None`: no hub.
pub fn link_stop(key: &str) -> Option<bool> {
    match rpc_within(
        Req::LinkStop {
            key: key.to_string(),
        },
        false,
        COMMAND_DEADLINE,
    ) {
        Some(Rep::Ok) => Some(true),
        Some(Rep::Links(_)) => Some(false),
        _ => None,
    }
}

/// Empty when no hub runs.
pub fn link_list() -> Vec<LinkRow> {
    match rpc_within(Req::LinkList, false, COMMAND_DEADLINE) {
        Some(Rep::Links(rows)) => rows,
        _ => Vec::new(),
    }
}
