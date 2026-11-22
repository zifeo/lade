//! The hub socket: connect, start a hub when asked, one request and its
//! reply under a deadline, and replace a hub that is stale.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::server::{lock_path, socket_path};
use super::wire::{self, ERR_DENIED, Rep, Req};

const DEADLINE: Duration = Duration::from_millis(20);
// An explicit command (a tunnel, `lade ttl set`) can wait for a cold hub to
// start. A cache lookup cannot.
pub(super) const COMMAND_DEADLINE: Duration = Duration::from_secs(2);
const STALE_CONFIRM: Duration = Duration::from_secs(1);

pub fn rpc(req: Req) -> Option<Rep> {
    rpc_inner(req, true)
}

pub(super) fn rpc_existing(req: Req) -> Option<Rep> {
    rpc_inner(req, false)
}

fn rpc_inner(req: Req, spawn: bool) -> Option<Rep> {
    // § e9ee1e4md.a@jyc3a
    rpc_within(req, spawn, DEADLINE)
}

/// `budget` covers spawning the hub, connecting, and the reply.
pub(super) fn rpc_within(req: Req, spawn: bool, budget: Duration) -> Option<Rep> {
    let deadline = Instant::now() + budget;
    let mut stream = connect(deadline, false, spawn)?;
    let (rep, after_write) = write_read(&mut stream, &req, deadline);
    if spawn && is_stale(&rep, after_write) {
        // § cex82a6ad.b@0hx4z
        if let Some(sock) = socket_path() {
            let _ = std::fs::remove_file(sock);
        }
        let deadline = Instant::now() + budget;
        let mut stream = connect(deadline, true, true)?;
        write_read(&mut stream, &req, deadline).0
    } else {
        rep
    }
}

pub(super) fn is_stale(rep: &Option<Rep>, after_write: bool) -> bool {
    // § cex82a6ad.a@z5mqb
    match rep {
        Some(Rep::Err(code)) => *code == ERR_DENIED,
        Some(_) => false,
        // A busy hub is not a wedged one, and replacing it stops its tunnels.
        None => after_write && !answers_ping(),
    }
}

fn answers_ping() -> bool {
    // § cex82a6ad.a@jg837
    let deadline = Instant::now() + STALE_CONFIRM;
    connect(deadline, false, false).is_some_and(|mut stream| {
        matches!(
            write_read(&mut stream, &Req::Ping, deadline).0,
            Some(Rep::Stat { .. })
        )
    })
}

fn connect(deadline: Instant, force_spawn: bool, allow_spawn: bool) -> Option<UnixStream> {
    remaining(deadline)?;
    // § hxw00jp6v.a@gw9p6
    let sock = socket_path()?;
    if !force_spawn && let Ok(stream) = UnixStream::connect(&sock) {
        apply_timeouts(&stream, deadline);
        return Some(stream);
    }
    if matches!(
        UnixStream::connect(&sock)
            .map(|_| ())
            .err()
            .map(|e| e.kind()),
        Some(std::io::ErrorKind::ConnectionRefused)
    ) {
        // § cex82a6ad@dhv0v
        let _ = std::fs::remove_file(&sock);
    }
    if !force_spawn
        && sock.exists()
        && let Ok(stream) = UnixStream::connect(&sock)
    {
        apply_timeouts(&stream, deadline);
        return Some(stream);
    }
    // § one f18xcggkd.b@q35br, f18xcggkd.c@qgrrd
    if !allow_spawn && !force_spawn {
        return None;
    }
    spawn_hub(deadline)?;
    loop {
        remaining(deadline)?;
        if let Ok(stream) = UnixStream::connect(&sock) {
            apply_timeouts(&stream, deadline);
            return Some(stream);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn spawn_hub(deadline: Instant) -> Option<()> {
    let exe = hub_exe()?;
    // § hxw00jp6v.c@0vhgc
    let file = lock_file()?;
    let flock = lock_within(file, deadline)?;
    if UnixStream::connect(socket_path()?).is_ok() {
        return Some(());
    }
    // § end hxw00jp6v.c
    // § hxw00jp6v.b@yjk40
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__hub")
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
    // § end hxw00jp6v.b
    drop(flock);
    Some(())
}

/// The start lock, waited for only until `deadline`, like the rest of a
/// request: another client starting the hub must not hold a lookup.
fn lock_within(
    mut file: std::fs::File,
    deadline: Instant,
) -> Option<nix::fcntl::Flock<std::fs::File>> {
    use nix::fcntl::{Flock, FlockArg};
    // § e9ee1e4md.a@mgfxh
    loop {
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(flock) => return Some(flock),
            Err((back, nix::errno::Errno::EAGAIN)) => file = back,
            Err(_) => return None,
        }
        remaining(deadline)?;
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn hub_exe() -> Option<PathBuf> {
    hub_exe_from(
        std::env::current_exe().ok()?,
        std::env::var_os("CARGO_BIN_EXE_lade"),
        cfg!(debug_assertions),
    )
}

/// Only a debug build, the one `cargo test` runs, takes the hub from
/// `CARGO_BIN_EXE_lade`: a test harness is not named `lade`, and a release
/// client must never start another program as its hub.
pub(super) fn hub_exe_from(
    exe: PathBuf,
    test_exe: Option<OsString>,
    debug: bool,
) -> Option<PathBuf> {
    // § hxw00jp6v.b@6ktf5
    if debug && let Some(path) = test_exe {
        return Some(PathBuf::from(path));
    }
    let named_lade = exe
        .file_name()
        .is_some_and(|name| name == "lade" || name == "lade.exe");
    named_lade.then_some(exe)
}

fn lock_file() -> Option<std::fs::File> {
    let path = lock_path()?;
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

fn write_read(stream: &mut UnixStream, req: &Req, deadline: Instant) -> (Option<Rep>, bool) {
    // § e9ee1e4md.a@vyxj1
    if remaining(deadline).is_none() {
        return (None, false);
    }
    apply_timeouts(stream, deadline);
    let Some(frame) = wire::encode_req(req) else {
        return (None, false);
    };
    if stream.write_all(&frame).is_err() {
        return (None, true);
    }
    if remaining(deadline).is_none() {
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

fn apply_timeouts(stream: &UnixStream, deadline: Instant) {
    if let Some(left) = remaining(deadline) {
        let _ = stream.set_read_timeout(Some(left));
        let _ = stream.set_write_timeout(Some(left));
    }
}

fn remaining(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
}
