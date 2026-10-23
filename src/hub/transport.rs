//! The hub socket: connect, start a hub when asked, one request and its
//! reply under a deadline, and replace a hub that is stale.

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
    rpc_within(req, spawn, DEADLINE)
}

/// `budget` covers spawning the hub, connecting, and the reply.
pub(super) fn rpc_within(req: Req, spawn: bool, budget: Duration) -> Option<Rep> {
    let deadline = Instant::now() + budget;
    let mut stream = connect(deadline, false, spawn)?;
    let (rep, after_write) = write_read(&mut stream, &req, deadline);
    if spawn && is_stale(&rep, after_write) {
        let _ = std::fs::remove_file(socket_path());
        let deadline = Instant::now() + budget;
        let mut stream = connect(deadline, true, true)?;
        write_read(&mut stream, &req, deadline).0
    } else {
        rep
    }
}

pub(super) fn is_stale(rep: &Option<Rep>, after_write: bool) -> bool {
    match rep {
        Some(Rep::Err(code)) => *code == ERR_DENIED,
        Some(_) => false,
        // A busy hub is not a wedged one, and replacing it stops its tunnels.
        None => after_write && !answers_ping(),
    }
}

fn answers_ping() -> bool {
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
    let sock = socket_path();
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
        let _ = std::fs::remove_file(&sock);
    }
    if !force_spawn
        && sock.exists()
        && let Ok(stream) = UnixStream::connect(&sock)
    {
        apply_timeouts(&stream, deadline);
        return Some(stream);
    }
    if !allow_spawn && !force_spawn {
        return None;
    }
    spawn_hub()?;
    loop {
        remaining(deadline)?;
        if let Ok(stream) = UnixStream::connect(&sock) {
            apply_timeouts(&stream, deadline);
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

fn write_read(stream: &mut UnixStream, req: &Req, deadline: Instant) -> (Option<Rep>, bool) {
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
