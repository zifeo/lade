use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::server::{lock_path, socket_path};
use super::wire::{self, CacheRow, ERR_DENIED, Rep, Req};

const DEADLINE: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HubState {
    Off,
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
}

impl HubInfo {
    fn off() -> Self {
        Self {
            state: HubState::Off,
            pid: None,
            secrets: 0,
            tickets: 0,
        }
    }

    fn down() -> Self {
        Self {
            state: HubState::Down,
            pid: None,
            secrets: 0,
            tickets: 0,
        }
    }

    fn stale() -> Self {
        Self {
            state: HubState::Stale,
            pid: None,
            secrets: 0,
            tickets: 0,
        }
    }

    fn up(pid: u32, secrets: u32, tickets: u32) -> Self {
        Self {
            state: HubState::Up,
            pid: Some(pid),
            secrets,
            tickets,
        }
    }
}

pub fn daemon_off() -> bool {
    std::env::var("LADE_DAEMON").ok().as_deref() == Some("off")
}

pub fn probe() -> HubInfo {
    if daemon_off() {
        return HubInfo::off();
    }
    match rpc_existing(Req::Ping) {
        Some(Rep::Stat {
            pid,
            secrets,
            tickets,
        }) => HubInfo::up(pid, secrets, tickets),
        Some(Rep::Err(code)) if code == ERR_DENIED => HubInfo::stale(),
        _ => HubInfo::down(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubStop {
    Off,
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
}

impl CacheListing {
    fn off() -> Self {
        Self {
            state: HubState::Off,
            pid: None,
            tickets: 0,
            rows: Vec::new(),
        }
    }

    fn down() -> Self {
        Self {
            state: HubState::Down,
            pid: None,
            tickets: 0,
            rows: Vec::new(),
        }
    }

    fn stale() -> Self {
        Self {
            state: HubState::Stale,
            pid: None,
            tickets: 0,
            rows: Vec::new(),
        }
    }

    fn up(pid: u32, tickets: u32, rows: Vec<CacheRow>) -> Self {
        Self {
            state: HubState::Up,
            pid: Some(pid),
            tickets,
            rows,
        }
    }
}

pub fn listing() -> CacheListing {
    if daemon_off() {
        return CacheListing::off();
    }
    match rpc_existing(Req::List) {
        Some(Rep::Listing { pid, tickets, rows }) => CacheListing::up(pid, tickets, rows),
        Some(Rep::Err(code)) if code == ERR_DENIED => CacheListing::stale(),
        _ => CacheListing::down(),
    }
}

pub fn scope(cwd: &std::path::Path) -> String {
    cwd.to_string_lossy().into_owned()
}

pub fn set_window(scope: &str, ttl: &str) -> Option<()> {
    match rpc(Req::SetWindow {
        scope: scope.to_string(),
        ttl: Some(ttl.to_string()),
    }) {
        Some(Rep::Ok) => Some(()),
        _ => None,
    }
}

pub fn unset_window(scope: &str) -> Option<()> {
    match rpc(Req::SetWindow {
        scope: scope.to_string(),
        ttl: None,
    }) {
        Some(Rep::Ok) => Some(()),
        _ => None,
    }
}

pub fn window(scope: &str) -> Option<String> {
    match rpc_existing(Req::GetWindow {
        scope: scope.to_string(),
    }) {
        Some(Rep::Window { ttl }) => ttl,
        _ => None,
    }
}

pub fn forget_names(names: &[String]) -> Option<()> {
    if names.is_empty() || daemon_off() {
        return None;
    }
    match rpc_existing(Req::Forget {
        names: names.to_vec(),
    }) {
        Some(Rep::Ok) => Some(()),
        _ => None,
    }
}

pub fn stop() -> HubStop {
    if daemon_off() {
        return HubStop::Off;
    }
    let result = match rpc_existing(Req::Quit) {
        Some(Rep::Ok) => HubStop::Stopped,
        Some(Rep::Err(code)) if code == ERR_DENIED => HubStop::Stale,
        _ => HubStop::Down,
    };
    let _ = std::fs::remove_file(socket_path());
    result
}

pub fn rpc(req: Req) -> Option<Rep> {
    rpc_inner(req, true)
}

fn rpc_existing(req: Req) -> Option<Rep> {
    rpc_inner(req, false)
}

fn rpc_inner(req: Req, spawn: bool) -> Option<Rep> {
    if daemon_off() {
        return None;
    }
    let started = Instant::now();
    let mut stream = connect(started, false, spawn)?;
    let (rep, after_write) = write_read(&mut stream, &req, started);
    if spawn && is_stale(&rep, after_write) {
        let _ = std::fs::remove_file(socket_path());
        let started = Instant::now();
        let mut stream = connect(started, true, true)?;
        write_read(&mut stream, &req, started).0
    } else {
        rep
    }
}

fn is_stale(rep: &Option<Rep>, after_write: bool) -> bool {
    matches!(rep, Some(Rep::Err(code)) if *code == ERR_DENIED) || (after_write && rep.is_none())
}

fn connect(started: Instant, force_spawn: bool, allow_spawn: bool) -> Option<UnixStream> {
    remaining(started)?;
    let sock = socket_path();
    if !force_spawn && let Ok(stream) = UnixStream::connect(&sock) {
        apply_timeouts(&stream, started);
        return Some(stream);
    }
    if matches!(
        UnixStream::connect(&sock)
            .map(|_| ())
            .err()
            .map(|e| e.kind()),
        Some(std::io::ErrorKind::ConnectionRefused)
    ) {
        let _ = std::fs::remove_file(&sock);
    }
    if !force_spawn
        && sock.exists()
        && let Ok(stream) = UnixStream::connect(&sock)
    {
        apply_timeouts(&stream, started);
        return Some(stream);
    }
    if !allow_spawn && !force_spawn {
        return None;
    }
    spawn_hub()?;
    loop {
        remaining(started)?;
        if let Ok(stream) = UnixStream::connect(&sock) {
            apply_timeouts(&stream, started);
            return Some(stream);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn spawn_hub() -> Option<()> {
    use nix::fcntl::{Flock, FlockArg};

    let exe = hub_exe()?;
    let file = lock_file()?;
    let flock = Flock::lock(file, FlockArg::LockExclusive).ok()?;
    if UnixStream::connect(socket_path()).is_ok() {
        return Some(());
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("hub")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    cmd.spawn().ok()?;
    drop(flock);
    Some(())
}

fn hub_exe() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_lade") {
        return Some(PathBuf::from(path));
    }
    let exe = std::env::current_exe().ok()?;
    let name = exe.file_name()?.to_string_lossy();
    if name == "lade" || name == "lade.exe" {
        Some(exe)
    } else {
        None
    }
}

fn lock_file() -> Option<std::fs::File> {
    let path = lock_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .ok()
}

fn write_read(stream: &mut UnixStream, req: &Req, started: Instant) -> (Option<Rep>, bool) {
    if remaining(started).is_none() {
        return (None, false);
    }
    apply_timeouts(stream, started);
    let Some(frame) = wire::encode_req(req) else {
        return (None, false);
    };
    if stream.write_all(&frame).is_err() {
        return (None, true);
    }
    if remaining(started).is_none() {
        return (None, true);
    }
    let mut header = [0u8; 8];
    if stream.read_exact(&mut header).is_err() {
        return (None, true);
    }
    let Some(n) = wire::header_len(&header) else {
        return (None, true);
    };
    let mut body = vec![0u8; n as usize];
    if stream.read_exact(&mut body).is_err() {
        return (None, true);
    }
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&body);
    (wire::decode_rep(&frame), true)
}

fn apply_timeouts(stream: &UnixStream, started: Instant) {
    if let Some(left) = remaining(started) {
        let _ = stream.set_read_timeout(Some(left));
        let _ = stream.set_write_timeout(Some(left));
    }
}

fn remaining(started: Instant) -> Option<Duration> {
    DEADLINE.checked_sub(started.elapsed())
}

pub fn put_t(id: &str) {
    if !crate::ticket::is_id(id) || daemon_off() {
        return;
    }
    let Ok(bytes) = std::fs::read(crate::ticket::path(id)) else {
        return;
    };
    let mut t = [0u8; 4];
    t.copy_from_slice(id.as_bytes());
    let _ = rpc_existing(Req::PutT { t, ticket: bytes });
}

pub fn unlink_t(id: &str) {
    if !crate::ticket::is_id(id) || daemon_off() {
        return;
    }
    let mut t = [0u8; 4];
    t.copy_from_slice(id.as_bytes());
    let _ = rpc_existing(Req::UnlinkT { t });
}
