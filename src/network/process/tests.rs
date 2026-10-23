use super::logs::{dedupe_lines, printed_addr};
use super::*;
use anyhow::anyhow;
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;

// Parallel unit tests mutate PATH via temp_env (mise stubs, etc.).
// Command::new("sh") then misses /bin/sh on macOS.
fn sh_command(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script]);
    command
}

fn fixed(port: u16) -> Inner {
    Inner::Fixed(Endpoint::Tcp {
        host: "127.0.0.1".to_string(),
        port,
    })
}

#[test]
fn dedupe_lines_collapses_kubernetes_memcache_retries() {
    let raw = concat!(
        "E0705 00:01:52.834950   86438 memcache.go:265] \"Unhandled Error\" err=\"same\"\n",
        "E0705 00:01:52.894498   86438 memcache.go:265] \"Unhandled Error\" err=\"same\"\n",
        "Unable to connect to the server: same\n",
    );

    let text = dedupe_lines(raw);
    assert_eq!(text.matches("memcache.go").count(), 1);
    assert!(text.contains("Unable to connect to the server: same"));
}

#[test]
fn printed_addr_reads_kubectl_and_tsh_ready_lines() {
    let addr = |s: &str| Some(s.parse::<std::net::SocketAddr>().unwrap());
    assert_eq!(
        printed_addr("Forwarding from 127.0.0.1:54321 -> 5432\n"),
        addr("127.0.0.1:54321")
    );
    assert_eq!(
        printed_addr("Forwarding from [::1]:54321 -> 5432"),
        addr("[::1]:54321")
    );
    assert_eq!(
        printed_addr("Proxying connections to acme-api:3000 on 127.0.0.1:54321\n"),
        addr("127.0.0.1:54321")
    );
    assert_eq!(printed_addr("Forwarding from 0.0.0.0:54321 -> 5432"), None);
    assert_eq!(printed_addr("Handling connection for 5432"), None);
}

#[test]
#[cfg(unix)]
fn dropping_supervisor_terminates_the_provider_process_group() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (forward, pid) = RunningForward::supervise(
        "test forward".to_string(),
        fixed(address.port()),
        Stop::default(),
        || Ok(sh_command("sleep 60")),
    )
    .unwrap();
    drop(forward);
    assert_eq!(kill(Pid::from_raw(pid as i32), None), Err(Errno::ESRCH));
}

#[test]
#[cfg(unix)]
fn supervisor_retries_a_startup_failure() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let command_attempts = Arc::clone(&attempts);
    let (forward, _) = RunningForward::supervise(
        "test forward".to_string(),
        fixed(address.port()),
        Stop::default(),
        move || {
            let attempt = command_attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                return Err(anyhow!("simulated startup failure"));
            }
            Ok(sh_command("sleep 60"))
        },
    )
    .unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    drop(forward);
}

#[test]
#[cfg(unix)]
fn supervisor_waits_for_the_printed_port() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let script = format!("echo 'Forwarding from 127.0.0.1:{port} -> 5432'; sleep 60");
    let (forward, _) = RunningForward::supervise(
        "test forward".to_string(),
        Inner::Printed,
        Stop::default(),
        move || Ok(sh_command(&script)),
    )
    .unwrap();
    assert_eq!(
        *forward.endpoint().lock().unwrap(),
        Some(Endpoint::Tcp {
            host: "127.0.0.1".to_string(),
            port,
        })
    );
    drop(forward);
}

#[test]
#[cfg(unix)]
fn drop_stops_a_provider_group_that_ignores_sigterm() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The background sleep inherits the ignored TERM and the log pipes.
    // Ready only once both are in place.
    let script =
        format!("trap '' TERM; sleep 60 & echo 'Forwarding from 127.0.0.1:{port} -> 1'; wait");
    let (forward, pid) = RunningForward::supervise(
        "test forward".to_string(),
        Inner::Printed,
        Stop::default(),
        move || Ok(sh_command(&script)),
    )
    .unwrap();
    let dropped = std::time::Instant::now();
    drop(forward);
    assert!(dropped.elapsed() < Duration::from_secs(5));
    assert_eq!(kill(Pid::from_raw(pid as i32), None), Err(Errno::ESRCH));
}

#[test]
#[cfg(unix)]
fn a_provider_restarts_past_a_helper_holding_its_logs() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let dir = tempfile::tempdir().unwrap();
    let runs = dir.path().join("runs");
    // Ready, then it exits and leaves a sleep on its stdout.
    let script = format!(
        "echo run >> {}; sleep 60 & echo 'Forwarding from 127.0.0.1:{port} -> 1'; sleep 0.3",
        runs.display()
    );
    let (forward, _) = RunningForward::supervise(
        "test forward".to_string(),
        Inner::Printed,
        Stop::default(),
        move || Ok(sh_command(&script)),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::fs::read_to_string(&runs)
        .unwrap_or_default()
        .lines()
        .count()
        < 2
    {
        assert!(std::time::Instant::now() < deadline, "never restarted");
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(forward);
}

#[test]
#[cfg(unix)]
fn stop_cancels_a_start_and_its_provider() {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let script = format!("echo $$ > {}; exec sleep 60", pid_file.display());
    let stop = Stop::default();
    let stopper = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        stopper.stop();
    });
    let started = std::time::Instant::now();
    let err = RunningForward::supervise("test forward".to_string(), fixed(port), stop, move || {
        Ok(sh_command(&script))
    })
    .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(err.to_string().contains("stopped before readiness"));
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(kill(Pid::from_raw(pid), None), Err(Errno::ESRCH));
}
