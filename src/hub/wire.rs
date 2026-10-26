use rkyv::{Archive, Deserialize, Serialize};

pub const MAGIC: &[u8; 4] = b"LDH1";
pub const MAX_BODY: u32 = 1_048_576;
pub const ERR_DENIED: u8 = 4;
pub const BIN_VER: &str = env!("CARGO_PKG_VERSION");

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct WireReq {
    ver: String,
    body: Req,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct WireRep {
    ver: String,
    body: Rep,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Req {
    Get {
        cwd: String,
        path: String,
        rule: String,
        when: String,
        walk: [u8; 32],
        user: String,
    },
    Put {
        cwd: String,
        path: String,
        rule: String,
        when: String,
        walk: [u8; 32],
        user: String,
        ttl_ms: u32,
        bindings: Vec<(String, Vec<u8>)>,
    },
    GetT {
        t: [u8; 4],
    },
    /// `notice`: a human shell also asks for the hourly notice slot. May
    /// start the version check.
    PutT {
        t: [u8; 4],
        ticket: Vec<u8>,
        notice: bool,
    },
    UnlinkT {
        t: [u8; 4],
    },
    Ping,
    Quit,
    List,
    Forget {
        names: Vec<String>,
    },
    /// `ttl` is `off` or a window. `None` clears this scope.
    SetWindow {
        scope: String,
        ttl: Option<String>,
    },
    GetWindow {
        scope: String,
    },
    /// Read the version ledger. Starts nothing.
    Version,
    /// Reuse or start a tunnel. `launch: None` only reuses.
    TunnelOpen(TunnelAsk),
    TunnelPoll {
        id: u64,
    },
    /// Drops this lease from every tunnel.
    TunnelRelease {
        lease: String,
    },
}

/// Who keeps a tunnel open: `id` is a ticket or `p<pid>`. `started` tells a
/// live `pid` from a reused one.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Lease {
    pub id: String,
    pub pid: u32,
    pub started: u64,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TunnelAsk {
    pub key: String,
    pub uri: String,
    pub host: String,
    /// `None`: an env-var key without `local=`. Reuse matches on `key`.
    pub port: Option<u16>,
    /// Two asks with the same fingerprint reach the same target.
    pub fingerprint: String,
    /// How long the tunnel outlives its last lease. `None`: it stops then.
    pub ttl_ms: Option<u32>,
    pub lease: Lease,
    pub launch: Option<TunnelLaunch>,
}

/// Built where the user's env lives: the provider runs with this env.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TunnelLaunch {
    pub context: Option<String>,
    pub env: Vec<(Vec<u8>, Vec<u8>)>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum TunnelRep {
    /// Nothing to reuse. Ask again with a launch.
    Absent,
    Starting {
        id: u64,
        port: u16,
    },
    Ready {
        id: u64,
        port: u16,
        pid: u32,
        reused: bool,
    },
    Failed {
        error: String,
    },
    /// `holder`: key and URI of the hub tunnel on that port. `None`: another
    /// process holds it.
    Conflict {
        port: u16,
        holder: Option<(String, String)>,
    },
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TunnelRow {
    pub key: String,
    pub uri: String,
    pub port: u16,
    pub leases: u32,
    /// `None` while a command uses it, or when it stops with its last lease.
    pub ttl_left_ms: Option<u32>,
    pub phase: TunnelPhase,
}

/// A failed start stays listed until the commands that asked for it let go.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelPhase {
    Starting,
    Ready,
    Failed,
}

/// What this hub knows about the latest release. RAM only.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionLedger {
    /// Unix ms of the last GitHub attempt, failed or not.
    pub tried_at_ms: Option<i64>,
    pub latest: Option<String>,
    pub error: Option<String>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Rep {
    Ok,
    Hit {
        bindings: Vec<(String, Vec<u8>)>,
    },
    Miss,
    Ticket {
        ticket: Vec<u8>,
    },
    Err(u8),
    Stat {
        pid: u32,
        secrets: u32,
        tickets: u32,
    },
    Listing {
        pid: u32,
        tickets: u32,
        rows: Vec<CacheRow>,
        tunnels: Vec<TunnelRow>,
        /// The `max_cache` this hub enforces, as its watcher last read it.
        max_cache_ms: Option<u32>,
    },
    Window {
        ttl: Option<String>,
    },
    /// Reply to a `PutT` with `notice`. `check`: this caller owns the
    /// hourly slot to check hooks and show notices.
    Notice {
        check: bool,
        version: VersionLedger,
    },
    Version(VersionLedger),
    Tunnel(TunnelRep),
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CacheRow {
    pub name: String,
    pub cwd: String,
    pub path: String,
    pub rule: String,
    pub when: String,
    pub user: String,
    pub ttl_left_ms: u32,
}

pub fn encode_req(value: &Req) -> Option<Vec<u8>> {
    encode_req_ver(value, BIN_VER)
}

pub fn encode_rep(value: &Rep) -> Option<Vec<u8>> {
    encode_rep_ver(value, BIN_VER)
}

pub(crate) fn encode_rep_ver(value: &Rep, ver: &str) -> Option<Vec<u8>> {
    wrap_archive(
        rkyv::to_bytes::<rkyv::rancor::Error>(&WireRep {
            ver: ver.to_string(),
            body: value.clone(),
        })
        .ok()?,
    )
}

pub(crate) fn encode_req_ver(value: &Req, ver: &str) -> Option<Vec<u8>> {
    wrap_archive(
        rkyv::to_bytes::<rkyv::rancor::Error>(&WireReq {
            ver: ver.to_string(),
            body: value.clone(),
        })
        .ok()?,
    )
}

fn wrap_archive(archive: impl AsRef<[u8]>) -> Option<Vec<u8>> {
    let archive = archive.as_ref();
    let n = u32::try_from(archive.len()).ok()?;
    if n == 0 || n > MAX_BODY {
        return None;
    }
    let mut out = Vec::with_capacity(8 + archive.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(archive);
    Some(out)
}

pub enum InReq {
    Body(Req),
    WrongVer,
}

pub fn decode_in_req(frame: &[u8]) -> Option<InReq> {
    let body = archive_body(frame)?;
    let env = rkyv::from_bytes::<WireReq, rkyv::rancor::Error>(body).ok()?;
    if env.ver != BIN_VER {
        Some(InReq::WrongVer)
    } else {
        Some(InReq::Body(env.body))
    }
}

pub fn decode_rep(frame: &[u8]) -> Option<Rep> {
    let body = archive_body(frame)?;
    let env = rkyv::from_bytes::<WireRep, rkyv::rancor::Error>(body).ok()?;
    if env.ver != BIN_VER {
        Some(Rep::Err(ERR_DENIED))
    } else {
        Some(env.body)
    }
}

pub fn header_len(header: &[u8; 8]) -> Option<u32> {
    if header[0..4] != MAGIC[..] {
        return None;
    }
    let n = u32::from_le_bytes(header[4..8].try_into().ok()?);
    if n == 0 || n > MAX_BODY {
        None
    } else {
        Some(n)
    }
}

fn archive_body(frame: &[u8]) -> Option<&[u8]> {
    if frame.len() < 8 {
        return None;
    }
    let n = header_len(frame[0..8].try_into().ok()?)?;
    frame.get(8..8 + n as usize)
}
