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

pub const READY: Duration = Duration::from_secs(8);
pub const EXIT: Duration = Duration::from_secs(5);

pub struct LadeChild {
    pub child: Child,
    pub extra: Vec<u32>,
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl LadeChild {
    pub fn spawn(mut child: Child) -> Self {
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

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn signal(&self, signal: Signal) {
        kill(Pid::from_raw(self.pid() as i32), signal).unwrap();
    }

    pub fn running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    pub fn wait_exit(&mut self) -> std::process::ExitStatus {
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

pub fn wait_exists(path: &Path) {
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

pub fn wait_log_has(path: &Path, needle: &str) {
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

pub fn read_pid(path: &Path) -> u32 {
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

pub fn session_of(pid: u32) -> pid_t {
    let sid = unsafe { libc::getsid(pid as pid_t) };
    assert!(sid > 0, "getsid({pid}) failed: {sid}");
    sid
}

pub fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as pid_t, 0) == 0 }
}

pub fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn trap_script(dir: &Path) -> PathBuf {
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

pub fn spawn_inject(
    home: &Path,
    cwd: &Path,
    script: &Path,
    work: &Path,
    no_mask: bool,
) -> LadeChild {
    let mut cmd = super::lade_std(home);
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

pub fn spawn_mcp(home: &Path, cwd: &Path, script: &Path, args: &[&str]) -> LadeChild {
    let mut cmd = super::lade_std(home);
    cmd.current_dir(cwd).arg("mcp").arg("--").arg(script);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    LadeChild::spawn(cmd.spawn().unwrap())
}

pub fn wait_ready(work: &Path, lade: &mut LadeChild) {
    wait_exists(&work.join("ready"));
    assert!(lade.running(), "lade exited before the child was ready");
}
