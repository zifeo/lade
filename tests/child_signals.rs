#![cfg(unix)]

mod common;

use common::child::*;
use nix::sys::signal::Signal;
use std::fs;
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
fn inject_child_stays_in_lade_session() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let script = trap_script(dir.path());
    let mut lade = spawn_inject(home.path(), dir.path(), &script, dir.path(), true);
    wait_ready(dir.path(), &mut lade);
    let child = read_pid(&dir.path().join("pid"));
    assert_eq!(session_of(lade.pid()), session_of(child));
}

#[test]
fn inject_ignores_hangup_and_keeps_child() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let script = trap_script(dir.path());
    let mut lade = spawn_inject(home.path(), dir.path(), &script, dir.path(), true);
    wait_ready(dir.path(), &mut lade);
    let child = read_pid(&dir.path().join("pid"));
    lade.signal(Signal::SIGHUP);
    std::thread::sleep(Duration::from_millis(300));
    assert!(lade.running(), "lade exited on SIGHUP");
    assert!(pid_alive(child), "inject child died on wrapper SIGHUP");
    let log = fs::read_to_string(dir.path().join("log")).unwrap_or_default();
    assert!(
        !log.lines().any(|line| line == "HUP"),
        "HUP was forwarded: {log}"
    );
}

#[test]
fn inject_forwards_usr_and_winch_and_stays_up() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let script = trap_script(dir.path());
    let mut lade = spawn_inject(home.path(), dir.path(), &script, dir.path(), true);
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
fn inject_piped_path_forwards_term() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let script = trap_script(dir.path());
    let mut lade = spawn_inject(home.path(), dir.path(), &script, dir.path(), false);
    wait_ready(dir.path(), &mut lade);
    lade.signal(Signal::SIGTERM);
    wait_log_has(&dir.path().join("log"), "TERM");
    let status = lade.wait_exit();
    assert!(!status.success(), "{status}");
}

#[test]
fn inject_stop_signals_end_wrapper_and_child() {
    for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGQUIT] {
        let dir = tempdir().unwrap();
        let home = tempdir().unwrap();
        let script = trap_script(dir.path());
        let mut lade = spawn_inject(home.path(), dir.path(), &script, dir.path(), true);
        wait_ready(dir.path(), &mut lade);
        let child = read_pid(&dir.path().join("pid"));
        lade.signal(signal);
        let status = lade.wait_exit();
        assert!(!status.success(), "{signal:?} status={status}");
        let started = Instant::now();
        while pid_alive(child) {
            assert!(
                started.elapsed() < EXIT,
                "{signal:?} left the inject child running"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
