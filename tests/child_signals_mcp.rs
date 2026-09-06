#![cfg(unix)]

mod common;

use common::child::*;
use nix::libc::pid_t;
use nix::sys::signal::Signal;
use std::fs;
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
fn mcp_child_gets_its_own_session() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let script = trap_script(dir.path());
    let mut lade = spawn_mcp(
        home.path(),
        dir.path(),
        &script,
        &[dir.path().to_str().unwrap()],
    );
    wait_ready(dir.path(), &mut lade);
    let child = read_pid(&dir.path().join("pid"));
    assert_ne!(session_of(lade.pid()), session_of(child));
    assert_eq!(session_of(child), child as pid_t);
}

#[test]
fn mcp_ignores_hangup_and_keeps_child() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let script = trap_script(dir.path());
    let mut lade = spawn_mcp(
        home.path(),
        dir.path(),
        &script,
        &[dir.path().to_str().unwrap()],
    );
    wait_ready(dir.path(), &mut lade);
    let child = read_pid(&dir.path().join("pid"));
    lade.signal(Signal::SIGHUP);
    std::thread::sleep(Duration::from_millis(300));
    assert!(lade.running(), "lade exited on SIGHUP");
    assert!(pid_alive(child), "MCP child died on wrapper SIGHUP");
    let log = fs::read_to_string(dir.path().join("log")).unwrap_or_default();
    assert!(
        !log.lines().any(|line| line == "HUP"),
        "HUP was forwarded: {log}"
    );
}

#[test]
fn mcp_forwards_usr_and_winch_and_stays_up() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let script = trap_script(dir.path());
    let mut lade = spawn_mcp(
        home.path(),
        dir.path(),
        &script,
        &[dir.path().to_str().unwrap()],
    );
    wait_ready(dir.path(), &mut lade);
    for signal in [Signal::SIGUSR1, Signal::SIGUSR2, Signal::SIGWINCH] {
        lade.signal(signal);
    }
    wait_log_has(&dir.path().join("log"), "USR1");
    wait_log_has(&dir.path().join("log"), "USR2");
    wait_log_has(&dir.path().join("log"), "WINCH");
    assert!(lade.running(), "lade exited after forward signals");
}

#[test]
fn mcp_stop_signals_end_wrapper_and_child() {
    for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGQUIT] {
        let dir = tempdir().unwrap();
        let home = tempdir().unwrap();
        let script = trap_script(dir.path());
        let mut lade = spawn_mcp(
            home.path(),
            dir.path(),
            &script,
            &[dir.path().to_str().unwrap()],
        );
        wait_ready(dir.path(), &mut lade);
        let child = read_pid(&dir.path().join("pid"));
        lade.signal(signal);
        let _ = lade.wait_exit();
        let started = Instant::now();
        while pid_alive(child) {
            assert!(
                started.elapsed() < EXIT,
                "{signal:?} left the MCP child running"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn mcp_term_kills_same_group_grandchild() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let script = dir.path().join("family.sh");
    write_executable(
        &script,
        r#"#!/bin/sh
dir=$1
echo $$ > "$dir/pid"
(
  echo $$ > "$dir/grand.pid"
  trap 'printf "%s\n" TERM >> "$dir/grand.log"; exit 0' TERM
  touch "$dir/grand.ready"
  while true; do sleep 0.2; done
) &
echo $! > "$dir/grand.watch"
touch "$dir/ready"
wait
"#,
    );
    let mut lade = spawn_mcp(
        home.path(),
        dir.path(),
        &script,
        &[dir.path().to_str().unwrap()],
    );
    wait_ready(dir.path(), &mut lade);
    wait_exists(&dir.path().join("grand.ready"));
    let grand = read_pid(&dir.path().join("grand.pid"));
    lade.extra.push(grand);
    lade.signal(Signal::SIGTERM);
    let _ = lade.wait_exit();
    wait_log_has(&dir.path().join("grand.log"), "TERM");
    let started = Instant::now();
    while pid_alive(grand) {
        assert!(
            started.elapsed() < EXIT,
            "MCP grandchild survived SIGTERM to lade"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn mcp_client_eof_during_pre_initialize_backoff_does_not_restart() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let starts = dir.path().join("starts");
    let script = dir.path().join("crash-once.sh");
    write_executable(
        &script,
        &format!(
            r#"#!/bin/sh
printf x >> '{starts}'
if [ ! -f '{started}' ]; then
  touch '{started}'
  exit 1
fi
exec cat
"#,
            starts = starts.display(),
            started = dir.path().join("started").display()
        ),
    );
    let mut lade = spawn_mcp(home.path(), dir.path(), &script, &[]);
    wait_exists(&dir.path().join("started"));
    drop(lade.child.stdin.take());
    let _ = lade.wait_exit();
    assert_eq!(fs::read_to_string(&starts).unwrap(), "x");
}

#[test]
fn mcp_term_after_initialize_does_not_restart() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let starts = dir.path().join("starts");
    let ready = dir.path().join("ready");
    let got_init = dir.path().join("got_init");
    let script = dir.path().join("hold.sh");
    write_executable(
        &script,
        &format!(
            r#"#!/bin/sh
printf x >> '{starts}'
touch '{ready}'
read line
touch '{got_init}'
exec cat
"#,
            starts = starts.display(),
            ready = ready.display(),
            got_init = got_init.display()
        ),
    );
    let mut lade = spawn_mcp(home.path(), dir.path(), &script, &[]);
    wait_exists(&ready);
    {
        let stdin = lade.child.stdin.as_mut().expect("piped stdin");
        use std::io::Write;
        stdin
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n")
            .unwrap();
        stdin.flush().unwrap();
    }
    wait_exists(&got_init);
    lade.signal(Signal::SIGTERM);
    let _ = lade.wait_exit();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(fs::read_to_string(&starts).unwrap(), "x");
}
