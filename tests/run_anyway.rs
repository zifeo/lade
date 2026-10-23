#![cfg(unix)]

//! After a failed hydrate, a human at a terminal has 5 s to press Enter and
//! run the command without Lade. Silence cancels it, and so does having no
//! terminal at all.

mod common;

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nix::pty::openpty;
use nix::sys::termios::tcgetattr;
use tempfile::tempdir;

const PROMPT: &str = "Press Enter within 5 seconds";
const FAILING_RULE: &str =
    "\"touch.*\":\n  KEY: \"file:///nonexistent/lade-missing.json?query=.a\"\n";

/// Runs `lade inject touch <marker>` on a fresh pty that is its controlling
/// terminal, so `/dev/tty` is that pty. `answer` is typed once the prompt
/// shows. Returns whether the command ran, the exit status, and the output.
fn run_on_a_terminal(answer: Option<&[u8]>) -> (bool, std::process::ExitStatus, String) {
    let home = tempdir().unwrap();
    let marker = home.path().join("ran");
    common::write_yml_raw(home.path(), FAILING_RULE);
    let pty = openpty(None, None).unwrap();
    let before = tcgetattr(&pty.master).unwrap().local_flags;
    let mut cmd = common::lade_std(home.path());
    cmd.current_dir(home.path())
        .args(["inject", &format!("touch {}", marker.display())])
        .stdin(Stdio::from(pty.slave.try_clone().unwrap()))
        .stdout(Stdio::from(pty.slave.try_clone().unwrap()))
        .stderr(Stdio::from(pty.slave.try_clone().unwrap()));
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid()?;
            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();
    drop(pty.slave);
    let mut master = File::from(pty.master);
    let mut reader = master.try_clone().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut captured = String::new();
    let mut answered = false;
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "lade never exited: {captured}");
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
            captured.push_str(&String::from_utf8_lossy(&chunk));
        }
        if let Some(answer) = answer
            && !answered
            && captured.contains(PROMPT)
        {
            master.write_all(answer).unwrap();
            answered = true;
        }
    };
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
        captured.push_str(&String::from_utf8_lossy(&chunk));
    }
    let after = tcgetattr(&master).unwrap().local_flags;
    assert_eq!(before, after, "terminal mode not restored: {captured}");
    (marker.exists(), status, captured)
}

#[test]
fn enter_runs_the_command_without_lade() {
    let (ran, status, output) = run_on_a_terminal(Some(b"\n"));
    assert!(output.contains(PROMPT), "{output}");
    assert!(ran, "{output}");
    assert!(status.success(), "{output}");
}

#[test]
fn no_answer_cancels_the_command() {
    let started = Instant::now();
    let (ran, status, output) = run_on_a_terminal(None);
    assert!(output.contains(PROMPT), "{output}");
    assert!(!ran, "{output}");
    assert_eq!(status.code(), Some(1), "{output}");
    assert!(started.elapsed() >= Duration::from_secs(5), "{output}");
}

#[test]
fn ctrl_c_cancels_the_command_at_once() {
    let started = Instant::now();
    let (ran, status, output) = run_on_a_terminal(Some(b"\x03"));
    assert!(output.contains(PROMPT), "{output}");
    assert!(!ran, "{output}");
    assert_eq!(status.code(), Some(130), "{output}");
    assert!(started.elapsed() < Duration::from_secs(5), "{output}");
}

#[test]
fn without_a_terminal_the_command_is_cancelled_at_once() {
    let home = tempdir().unwrap();
    let marker = home.path().join("ran");
    common::write_yml_raw(home.path(), FAILING_RULE);
    let started = Instant::now();
    let output = common::lade_std(home.path())
        .current_dir(home.path())
        .args(["inject", &format!("touch {}", marker.display())])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(!marker.exists(), "{stderr}");
    assert!(!stderr.contains(PROMPT), "{stderr}");
    assert!(started.elapsed() < Duration::from_secs(5), "{stderr}");
}
