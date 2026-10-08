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
    PutT {
        t: [u8; 4],
        ticket: Vec<u8>,
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
    },
    Window {
        ttl: Option<String>,
    },
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CacheRow {
    pub name: String,
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
