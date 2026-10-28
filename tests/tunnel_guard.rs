#![cfg(unix)]

mod common;

use std::process::Stdio;
use std::time::{Duration, Instant};

use common::child::{pid_alive, read_pid};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use tempfile::tempdir;

fn exits_within(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
    false
}

#[test]
fn test_tunnel_guard_passes_the_provider_exit_code() {
    let home = tempdir().unwrap();
    let mut guard = common::lade_std(home.path())
        .args(["__tunnel-guard", "--", "/bin/sh", "-c", "exit 3"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    // `wait` would close stdin first, which reads as Lade being gone.
    let _lade = guard.stdin.take();
    assert_eq!(guard.wait().unwrap().code(), Some(3));
}

#[test]
fn test_tunnel_guard_stops_the_provider_once_its_lade_is_gone() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let script = format!("echo $$ > {}; exec sleep 60", pid_file.display());
    let mut guard = common::lade_std(home.path())
        .args(["__tunnel-guard", "--", "/bin/sh", "-c", &script])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let provider = read_pid(&pid_file);

    // What the kernel does to the pipe when the Lade holding it dies.
    drop(guard.stdin.take());

    let _ = guard.wait();
    assert!(exits_within(provider, Duration::from_secs(5)));
}

#[test]
fn test_killed_lade_leaves_no_provider_behind() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let bin = tempdir().unwrap();
    common::write_fake_ssh(bin.path());
    common::write_yml_raw(
        dir.path(),
        "\"sleep.*\":\n  DB_PORT: ssh://jump.example.com/db.internal/5432\n",
    );
    let pid_file = dir.path().join("ssh.pid");
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap()
    );
    let mut lade = common::lade_std(home.path())
        .current_dir(dir.path())
        .env("PATH", path)
        .env("FAKE_SSH_PID", &pid_file)
        .args(["inject", "sleep 20"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let provider = read_pid(&pid_file);
    for _ in 0..15 {
        assert!(pid_alive(provider), "provider died with Lade alive");
        std::thread::sleep(Duration::from_millis(20));
    }

    lade.kill().unwrap();
    lade.wait().unwrap();

    assert!(exits_within(provider, Duration::from_secs(5)));
}
