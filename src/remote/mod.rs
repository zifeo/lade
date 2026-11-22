//! Remote resolution: `lade connect` on a laptop, `lade attach` on the
//! remote, `lade __remote` between them, `lade __linked-tunnel` for each
//! tunnel of the remote. See `docs/remote.md`.

pub(crate) mod attach;
pub(crate) mod connect;
pub(crate) mod files;
pub(crate) mod link;
pub(crate) mod resolve;
pub(crate) mod streams;
pub(crate) mod transport;
pub(crate) mod tunnel;

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Audience;

/// `<cache root>/up`, 0700: the socket, its lock, and the marker.
pub(crate) fn dir() -> PathBuf {
    crate::cache::root().join("up")
}

pub(crate) fn socket_path() -> PathBuf {
    dir().join("sock")
}

pub(crate) fn lock_path() -> PathBuf {
    dir().join("lock")
}

pub(crate) fn marker_path() -> PathBuf {
    dir().join("attached.json")
}

/// Left by `lade detach` here, so a laptop hub that retries its link does
/// not attach this machine again. A fresh `lade connect` clears it.
pub(crate) fn detached_path() -> PathBuf {
    dir().join("detached")
}

/// Written by `lade attach`, kept while the link is down, removed by
/// `lade detach`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Marker {
    pub laptop: String,
    pub since: String,
    pub protocol: u32,
    /// The transport as the laptop's `lade connect` wrote it.
    pub transport: String,
    pub pid: u32,
}

pub(crate) enum MarkerState {
    None,
    Attached(Marker),
    Unreadable(String),
}

pub(crate) fn read_marker() -> MarkerState {
    let path = marker_path();
    match std::fs::read(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => MarkerState::None,
        Err(e) => MarkerState::Unreadable(format!("cannot read {}: {e}", path.display())),
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(marker) => MarkerState::Attached(marker),
            Err(e) => MarkerState::Unreadable(format!("cannot parse {}: {e}", path.display())),
        },
    }
}

pub(crate) fn write_marker(marker: &Marker) -> Result<()> {
    let path = marker_path();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(marker)?)
        .with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

pub(crate) fn ensure_dir() -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

static WALK: Mutex<Option<[u8; 32]>> = Mutex::new(None);
static AUDIENCE: OnceLock<Audience> = OnceLock::new();

/// The walk of the config this process loaded. A link request names it so
/// the laptop answers from its resolution of those files.
pub(crate) fn set_walk(walk: [u8; 32]) {
    *WALK.lock().unwrap_or_else(|e| e.into_inner()) = Some(walk);
}

fn walk_hex() -> Result<String> {
    let walk = *WALK.lock().unwrap_or_else(|e| e.into_inner());
    let walk = match walk {
        Some(walk) => walk,
        None => {
            let files = crate::config::yaml_files_on_walk(&std::env::current_dir()?)?;
            crate::config::hash_yaml_files(&files)
        }
    };
    let user = crate::global_config::GlobalConfig::resolved_user()?;
    Ok(hex(&link_walk(walk, &user)))
}

/// What names a walk on the link: its files and the saved Lade user, which
/// picks the per-user entries the laptop resolved, so a `lade user` change
/// needs a new `lade connect` as an edit does.
pub(crate) fn link_walk(walk: [u8; 32], user: &Option<String>) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(walk);
    match user {
        Some(user) => {
            hasher.update([1]);
            hasher.update(user.as_bytes());
        }
        None => hasher.update([0]),
    }
    hasher.finalize().into()
}

fn report() -> serde_json::Value {
    serde_json::json!({
        "argv": std::env::args().collect::<Vec<_>>(),
        "cwd": std::env::current_dir().ok(),
        "audience": AUDIENCE.get().map(Audience::as_str),
    })
}

/// Text the remote wrote, safe on the laptop's terminal: a control or
/// direction character would let it redraw the files `lade connect` shows.
pub(crate) fn shown(text: &str) -> String {
    text.chars()
        .map(|c| {
            let hidden = c.is_control()
                || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}');
            if hidden {
                format!("\\u{{{:x}}}", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The walk hash as `lade connect` shows it.
pub(crate) fn short_walk(walk_hex: &str) -> &str {
    &walk_hex[..16.min(walk_hex.len())]
}

/// On an attached machine, every secret provider resolves through the link
/// from here on, and never falls back to a session of its own.
pub(crate) fn install(audience: Audience) {
    // § a36hqp4ep@gsk84
    let _ = AUDIENCE.set(audience);
    let (laptop, connect, broken) = match read_marker() {
        MarkerState::None => return,
        MarkerState::Attached(marker) => (marker.laptop, connect_line(&marker.transport), None),
        MarkerState::Unreadable(why) => (
            "a laptop".to_string(),
            "lade connect".to_string(),
            Some(why),
        ),
    };
    lade_sdk::upstream::attach(lade_sdk::upstream::Attached {
        socket: socket_path(),
        laptop,
        connect,
        broken,
        walk: walk_hex,
        report,
    });
}

/// The `lade connect` to run on the laptop for this directory.
pub(crate) fn connect_line(transport: &str) -> String {
    let dir = std::env::current_dir()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "<this directory>".to_string());
    format!("lade connect {} -- {transport}", transport::quote(&dir))
}

/// The walk hash a link request names, from its hex.
pub(crate) fn unhex(walk: &str) -> Option<[u8; 32]> {
    let bytes = (0..walk.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(walk.get(at..at + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    bytes.try_into().ok()
}

/// The exit code of a failed link request found in `error`'s chain.
pub(crate) fn exit_code(error: &anyhow::Error) -> Option<(i32, String)> {
    error.chain().find_map(|cause| {
        cause
            .downcast_ref::<lade_sdk::upstream::UpstreamError>()
            .map(|e| {
                let code = match e.code {
                    lade_sdk::upstream::ABSENT => crate::exit_codes::UPSTREAM_ABSENT,
                    lade_sdk::upstream::REFUSED => crate::exit_codes::UPSTREAM_REFUSED,
                    lade_sdk::upstream::WALK_UNKNOWN => crate::exit_codes::WALK_UNKNOWN,
                    _ => crate::exit_codes::FAILURE,
                };
                (code, e.message.clone())
            })
    })
}

/// `user@host` of this machine, the name a remote knows its laptop by.
/// From the account, not `USER`: the hub runs the link with a trimmed env,
/// and the name must match the one `lade connect` gave the remote.
pub(crate) fn machine_name() -> String {
    let uid = nix::unistd::getuid();
    let user = nix::unistd::User::from_uid(uid)
        .ok()
        .flatten()
        .map_or_else(|| format!("uid {uid}"), |user| user.name);
    format!("{user}@{}", hostname())
}

pub(crate) fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer outlives the call and its length is passed.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return "?".to_string();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn remote_text_cannot_move_the_cursor() {
        assert_eq!(
            super::shown("A: op://v/i/f\r\u{1b}[2A\u{1b}[J\u{202e}x\tok"),
            "A: op://v/i/f\\u{d}\\u{1b}[2A\\u{1b}[J\\u{202e}x\\u{9}ok"
        );
        assert_eq!(super::shown("plain café"), "plain café");
    }

    #[test]
    fn a_walk_hash_reads_back_from_its_hex() {
        let walk = [7u8; 32];
        assert_eq!(super::unhex(&super::hex(&walk)), Some(walk));
        assert_eq!(super::unhex("07"), None);
        assert_eq!(super::unhex(&"zz".repeat(32)), None);
    }
}
