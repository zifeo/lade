#![cfg(unix)]

mod common;

use nix::libc::{self, pid_t};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

const READY: Duration = Duration::from_secs(8);
const EXIT: Duration = Duration::from_secs(5);

struct LadeChild {
    child: Child,
    extra: Vec<u32>,
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl LadeChild {
    fn spawn(mut child: Child) -> Self {
        let stderr = Arc::new(Mutex::new(Vec::new()));
        if let Some(mut pipe) = child.stdout.take() {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
            });
        }
        if let Some(mut pipe) = child.stderr.take() {
            let stderr = Arc::clone(&stderr);
            thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                stderr.lock().unwrap().extend_from_slice(&buf);
            });
        }
        Self {
            child,
            extra: Vec::new(),
            stderr,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn signal(&self, signal: Signal) {
        kill(Pid::from_raw(self.pid() as i32), signal).unwrap();
    }

    fn running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    fn wait_exit(&mut self) -> std::process::ExitStatus {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            if started.elapsed() >= EXIT {
                let _ = kill(Pid::from_raw(self.pid() as i32), Signal::SIGKILL);
                let _ = self.child.wait();
                let stderr = self.stderr.lock().unwrap().clone();
                panic!(
                    "lade did not exit after stop signal; stderr={}",
                    String::from_utf8_lossy(&stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for LadeChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = kill(Pid::from_raw(self.pid() as i32), Signal::SIGKILL);
            let _ = self.child.wait();
        }
        for pid in &self.extra {
            let _ = kill(Pid::from_raw(*pid as i32), Signal::SIGKILL);
        }
    }
}

fn wait_exists(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < READY,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_log_has(path: &Path, needle: &str) {
    let started = Instant::now();
    loop {
        let body = fs::read_to_string(path).unwrap_or_default();
        if body.lines().any(|line| line == needle) {
            return;
        }
        assert!(
            started.elapsed() < READY,
            "timed out waiting for {needle} in {}: {body}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn read_pid(path: &Path) -> u32 {
    wait_exists(path);
    let started = Instant::now();
    loop {
        if let Ok(raw) = fs::read_to_string(path)
            && let Ok(pid) = raw.trim().parse::<u32>()
            && pid > 0
        {
            return pid;
        }
        assert!(
            started.elapsed() < READY,
            "timed out reading pid from {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn session_of(pid: u32) -> pid_t {
    let sid = unsafe { libc::getsid(pid as pid_t) };
    assert!(sid > 0, "getsid({pid}) failed: {sid}");
    sid
}

fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as pid_t, 0) == 0 }
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn trap_script(dir: &Path) -> PathBuf {
    let path = dir.join("trap.sh");
    write_executable(
        &path,
        r#"#!/bin/sh
dir=$1
echo $$ > "$dir/pid"
trap 'printf "%s\n" HUP >> "$dir/log"' HUP
trap 'printf "%s\n" INT >> "$dir/log"; exit 130' INT
trap 'printf "%s\n" TERM >> "$dir/log"; exit 143' TERM
trap 'printf "%s\n" QUIT >> "$dir/log"; exit 131' QUIT
trap 'printf "%s\n" USR1 >> "$dir/log"' USR1
trap 'printf "%s\n" USR2 >> "$dir/log"' USR2
trap 'printf "%s\n" WINCH >> "$dir/log"' WINCH
touch "$dir/ready"
while true; do sleep 0.2; done
"#,
    );
    path
}

fn spawn_inject(home: &Path, cwd: &Path, script: &Path, work: &Path, no_mask: bool) -> LadeChild {
    let mut cmd = common::lade_std(home);
    cmd.current_dir(cwd);
    if no_mask {
        cmd.args(["inject", "--no-mask", "--", "exec"]);
    } else {
        cmd.args(["inject", "--", "exec"]);
    }
    cmd.arg(script).arg(work);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    LadeChild::spawn(cmd.spawn().unwrap())
}

fn spawn_mcp(home: &Path, cwd: &Path, script: &Path, args: &[&str]) -> LadeChild {
    let mut cmd = common::lade_std(home);
    cmd.current_dir(cwd).arg("mcp").arg("--").arg(script);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    LadeChild::spawn(cmd.spawn().unwrap())
}

fn wait_ready(work: &Path, lade: &mut LadeChild) {
    wait_exists(&work.join("ready"));
    assert!(lade.running(), "lade exited before the child was ready");
}

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
    std::thread::sleep(Duration::from_millis(80));
    drop(lade.child.stdin.take());
    let _ = lade.wait_exit();
    std::thread::sleep(Duration::from_millis(400));
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
