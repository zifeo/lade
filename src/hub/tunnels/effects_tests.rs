use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;

use super::Tunnels;
use crate::hub::server::should_exit_for_test;
use crate::hub::tables::{empty_tables, handle_for_test};
use crate::hub::wire::{Lease, Rep, Req, TunnelAsk, TunnelLaunch, TunnelRep};
use crate::network::{free_fixed_port, process_started};

fn write_kubectl(dir: &Path, body: &str) {
    let script = dir.join("kubectl");
    std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A kubectl that records its pid and never prints a port, so its tunnel
/// stays starting.
fn stuck_kubectl(dir: &Path) {
    write_kubectl(
        dir,
        &format!("echo $$ > {}/pid\nexec sleep 60", dir.display()),
    );
}

fn lease(id: &str) -> Lease {
    Lease {
        id: id.to_string(),
        pid: 1,
        started: 0,
    }
}

fn ask(dir: &Path, port: u16, lease: Lease) -> TunnelAsk {
    let path = format!("{}:/bin:/usr/bin", dir.display());
    TunnelAsk {
        key: port.to_string(),
        uri: "kubectl://kube.example.com:443/acme/default/service/db/5432".to_string(),
        host: "127.0.0.1".to_string(),
        port: Some(port),
        fingerprint: "acme".to_string(),
        ttl_ms: None,
        lease,
        launch: Some(TunnelLaunch {
            context: Some("acme".to_string()),
            env: vec![(b"PATH".to_vec(), path.into_bytes())],
            link_walk: None,
        }),
    }
}

/// Starts a tunnel on a free port. A test running alongside may take the
/// port first: a conflict with no holder retries on a new one.
fn open_free(mut open: impl FnMut(TunnelAsk) -> TunnelRep, dir: &Path, lease: Lease) -> (u16, u64) {
    for _ in 0..8 {
        let port = free_fixed_port();
        match open(ask(dir, port, lease.clone())) {
            TunnelRep::Starting { id, .. } => return (port, id),
            TunnelRep::Conflict { holder: None, .. } => continue,
            rep => panic!("{rep:?}"),
        }
    }
    panic!("no free port in 8 tries");
}
fn provider_pid(dir: &Path) -> Pid {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(dir.join("pid"))
            && let Ok(pid) = text.trim().parse()
        {
            return Pid::from_raw(pid);
        }
        assert!(Instant::now() < deadline, "provider never started");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn exits_within(pid: Pid, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if kill(pid, None) == Err(Errno::ESRCH) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Children of this test binary that still run its image: forked, `exec`
/// not reached yet. No test spawns the test binary itself.
fn unexeced_forks() -> usize {
    let exe = std::env::current_exe().unwrap();
    children()
        .into_iter()
        .filter(|child| child_exe(*child).as_deref() == Some(exe.as_path()))
        .count()
}

#[cfg(target_os = "macos")]
fn children() -> Vec<i32> {
    let mut pids = vec![0i32; 4096];
    let size = i32::try_from(pids.len() * size_of::<i32>()).unwrap();
    let me = i32::try_from(std::process::id()).unwrap();
    let count = unsafe { libc::proc_listchildpids(me, pids.as_mut_ptr().cast(), size) };
    pids.truncate(usize::try_from(count).expect("proc_listchildpids"));
    pids
}

#[cfg(target_os = "macos")]
fn child_exe(pid: i32) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let size = u32::try_from(buf.len()).unwrap();
    let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), size) };
    let len = usize::try_from(len).ok().filter(|len| *len > 0)?;
    Some(std::ffi::OsStr::from_bytes(&buf[..len]).into())
}

#[cfg(target_os = "linux")]
fn children() -> Vec<i32> {
    let me = std::process::id().to_string();
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            // `pid (comm) state ppid ...`: comm may hold spaces or `)`.
            std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
                stat.rsplit_once(')')
                    .and_then(|(_, rest)| rest.split_whitespace().nth(1))
                    == Some(me.as_str())
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn child_exe(pid: i32) -> Option<std::path::PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[test]
fn hub_exit_stops_a_starting_provider() {
    let dir = tempfile::tempdir().unwrap();
    stuck_kubectl(dir.path());
    let mut tunnels = Tunnels::default();
    let (port, _) = open_free(|ask| tunnels.open(ask), dir.path(), lease("p1"));
    let pid = provider_pid(dir.path());

    let dropped = Instant::now();
    drop(tunnels);

    assert!(dropped.elapsed() < Duration::from_secs(5));
    assert_eq!(kill(pid, None), Err(Errno::ESRCH));
    // A test running alongside may get this ephemeral port from the kernel
    // for a moment.
    let deadline = Instant::now() + Duration::from_secs(2);
    while TcpListener::bind(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "port {port} still held");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn releasing_a_starting_tunnel_stops_its_provider() {
    let dir = tempfile::tempdir().unwrap();
    stuck_kubectl(dir.path());
    let mut tunnels = Tunnels::default();
    open_free(|ask| tunnels.open(ask), dir.path(), lease("p1"));
    let pid = provider_pid(dir.path());

    tunnels.release("p1", Duration::ZERO);

    assert!(exits_within(pid, Duration::from_secs(5)));
    assert!(tunnels.is_empty());
}

#[test]
fn a_busy_hub_stops_a_tunnel_whose_holder_died() {
    let dir = tempfile::tempdir().unwrap();
    stuck_kubectl(dir.path());
    let mut holder = Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let held = Lease {
        id: "holder".to_string(),
        pid: holder.id(),
        started: process_started(holder.id()).unwrap(),
    };
    let mut tables = empty_tables();
    let open = |ask| match handle_for_test(&mut tables, Req::TunnelOpen(ask)) {
        Rep::Tunnel(rep) => rep,
        rep => panic!("{rep:?}"),
    };
    open_free(open, dir.path(), held);
    let pid = provider_pid(dir.path());
    holder.kill().unwrap();
    holder.wait().unwrap();

    // The serve loop's naps, with no request in between.
    let deadline = Instant::now() + Duration::from_secs(5);
    while kill(pid, None) != Err(Errno::ESRCH) {
        assert!(Instant::now() < deadline, "provider still running");
        assert!(!should_exit_for_test(&mut tables, true, false));
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_released_tunnel_frees_its_port_before_release_returns() {
    let dir = tempfile::tempdir().unwrap();
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    // Ready at once: it prints the port of a listener this test holds.
    write_kubectl(
        dir.path(),
        &format!("echo 'Forwarding from 127.0.0.1:{upstream_port} -> 5432'\nexec sleep 60"),
    );
    let mut tunnels = Tunnels::default();
    let (port, id) = open_free(|ask| tunnels.open(ask), dir.path(), lease("p1"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(tunnels.poll(id), TunnelRep::Ready { .. }) {
        assert!(Instant::now() < deadline, "never ready");
        std::thread::sleep(Duration::from_millis(20));
    }

    tunnels.release("p1", Duration::ZERO);

    // A child another test forked keeps a copy of the closed listener until
    // it runs `exec`. Checked before the bind: a fork that already ran
    // `exec` can no longer hold the port.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let forks = unexeced_forks();
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(
            forks > 0 && Instant::now() < deadline,
            "port {port} still held"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_cancelled_start_makes_the_next_ask_wait_instead_of_failing() {
    let dir = tempfile::tempdir().unwrap();
    stuck_kubectl(dir.path());
    let mut tunnels = Tunnels::default();
    let (port, _) = open_free(|ask| tunnels.open(ask), dir.path(), lease("p1"));
    provider_pid(dir.path());

    // The start thread still holds the port while it stops the provider.
    tunnels.release("p1", Duration::ZERO);
    let rep = tunnels.open(ask(dir.path(), port, lease("p2")));

    assert!(
        !matches!(rep, TunnelRep::Conflict { holder: None, .. }),
        "{rep:?}"
    );
}

#[test]
fn a_restart_keeps_the_env_of_the_first_command() {
    let dir = tempfile::tempdir().unwrap();
    let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let marks = dir.path().join("marks");
    write_kubectl(
        dir.path(),
        &format!(
            "echo \"$MARK\" >> {marks}\necho $$ > {dir}/pid\necho 'Forwarding from 127.0.0.1:{upstream_port} -> 5432'\nexec sleep 60",
            marks = marks.display(),
            dir = dir.path().display()
        ),
    );
    let with_mark = |mut ask: TunnelAsk, mark: &str| {
        let launch = ask.launch.as_mut().unwrap();
        launch
            .env
            .push((b"MARK".to_vec(), mark.as_bytes().to_vec()));
        ask
    };
    let mut tunnels = Tunnels::default();
    let (port, id) = open_free(
        |ask| tunnels.open(with_mark(ask, "a")),
        dir.path(),
        lease("p1"),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(tunnels.poll(id), TunnelRep::Ready { .. }) {
        assert!(Instant::now() < deadline, "never ready");
        std::thread::sleep(Duration::from_millis(20));
    }
    let reused = tunnels.open(with_mark(ask(dir.path(), port, lease("p2")), "b"));
    assert!(
        matches!(reused, TunnelRep::Ready { reused: true, .. }),
        "{reused:?}"
    );

    let first = provider_pid(dir.path());
    std::fs::remove_file(dir.path().join("pid")).unwrap();
    kill(first, nix::sys::signal::Signal::SIGKILL).unwrap();
    let restarted = provider_pid(dir.path());
    assert_ne!(first, restarted);
    let text = std::fs::read_to_string(&marks).unwrap();
    drop(tunnels);
    assert_eq!(text, "a\na\n");
}

/// A kubefwd that listens on its `-m` port until `exit` appears in `dir`,
/// then exits. Every restart runs `restart` instead, so it never takes the
/// port back.
fn write_kubefwd(dir: &Path, restart: &str) {
    let script = dir.join("kubefwd");
    let body = format!(
        r#"#!/bin/sh
if [ -e {dir}/ran ]; then {restart}; fi
: > {dir}/ran
port=0
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-m" ]; then
    port="${{2##*:}}"
    shift 2
    continue
  fi
  shift
done
exec /usr/bin/python3 -c 'import os,socket,sys,time
s=socket.socket()
s.bind(("127.0.0.1", int(sys.argv[1])))
s.listen()
while not os.path.exists(sys.argv[2]):
    time.sleep(0.02)
' "$port" {dir}/exit
"#,
        dir = dir.display()
    );
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn kubefwd_ask(dir: &Path, port: u16, lease: Lease) -> TunnelAsk {
    TunnelAsk {
        uri: "kubefwd://kube.example.com:443/acme/default/service/db/5432".to_string(),
        ..ask(dir, port, lease)
    }
}

#[test]
fn a_kubefwd_tunnel_that_loses_its_port_is_not_reused() {
    a_lost_port_fails_the_tunnel("exec sleep 60");
}

#[test]
fn a_kubefwd_restart_that_cannot_bind_fails_the_tunnel() {
    a_lost_port_fails_the_tunnel("exit 1");
}

fn a_lost_port_fails_the_tunnel(restart: &str) {
    let dir = tempfile::tempdir().unwrap();
    write_kubefwd(dir.path(), restart);
    let mut tunnels = Tunnels::default();
    let (port, id) = open_free(
        |ask| tunnels.open(kubefwd_ask(dir.path(), ask.port.unwrap(), ask.lease)),
        dir.path(),
        lease("p1"),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(tunnels.poll(id), TunnelRep::Ready { .. }) {
        assert!(Instant::now() < deadline, "never ready");
        std::thread::sleep(Duration::from_millis(20));
    }

    std::fs::write(dir.path().join("exit"), b"").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let stranger = loop {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            break listener;
        }
        assert!(Instant::now() < deadline, "the provider never let go");
        std::thread::sleep(Duration::from_millis(10));
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(tunnels.poll(id), TunnelRep::Failed { .. }) {
        assert!(Instant::now() < deadline, "the hub never saw the loss");
        std::thread::sleep(Duration::from_millis(50));
    }

    let rep = tunnels.open(kubefwd_ask(dir.path(), port, lease("p2")));
    drop(stranger);
    assert!(
        matches!(rep, TunnelRep::Conflict { holder: None, .. }),
        "{rep:?}"
    );
}
