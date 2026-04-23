#![cfg(unix)]

//! End-to-end PTY harness for `lade inject`. Allocates a pseudo-terminal,
//! spawns lade inside it with a trivial long-running child, and plays the
//! role of the terminal emulator by injecting standard terminal responses
//! on stdin:
//!
//! - OSC 11 — xterm "report background color" reply.
//! - CPR   — ECMA-48 DSR-6 cursor position reply.
//!
//! These are emitted by any TUI library that probes the terminal (tofu's
//! Go stack is one example, not a prerequisite for the bug). The assertion
//! is that those bytes, once injected on stdin, are not re-surfaced on
//! lade's stdout / visible to the user — which matches the original report:
//! `^[]11;rgb:…^[\^[[…R`.
//!
//! `nix::pty::openpty` is used directly so the test controls the slave's
//! termios explicitly. The real user-facing TTY runs in cooked mode with
//! ECHO on; that is the mode where the bug manifests.

use nix::pty::{OpenptyResult, Winsize, openpty};
use nix::sys::termios::{
    ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg, SpecialCharacterIndices, Termios,
    cfsetispeed, cfsetospeed, tcsetattr,
};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

// ECMA-48 / xterm standard response forms.
const OSC_RESPONSE: &[u8] = b"\x1b]11;rgb:1f1f/2424/2828\x1b\\";
const CPR_RESPONSE: &[u8] = b"\x1b[37;1R";

// Distinctive byte sequences in the responses — looking for these in the
// captured output tells us the injected stdin bytes were echoed through.
const OSC_MARKER: &[u8] = b"rgb:1f1f/2424/2828";
const CPR_MARKER: &[u8] = b";1R";

// Printed by the child before sleeping so the harness can synchronise on
// lade being in its PTY loop before injecting responses on stdin.
const CHILD_READY: &str = "__lade_pty_ready__";

fn cooked_termios() -> Termios {
    // Build a termios from scratch matching a typical login tty on Linux /
    // macOS: canonical input, ECHO on, sane output post-processing. Using
    // `tcgetattr` on the freshly-opened slave would also work, but the
    // exact defaults vary by libc — hardcoding is less flaky.
    let master = openpty(None, None).unwrap();
    let mut t = nix::sys::termios::tcgetattr(&master.slave).unwrap();
    drop(master);

    t.input_flags |= InputFlags::BRKINT
        | InputFlags::ICRNL
        | InputFlags::IMAXBEL
        | InputFlags::IXON
        | InputFlags::IUTF8;
    t.output_flags |= OutputFlags::OPOST | OutputFlags::ONLCR;
    t.control_flags |= ControlFlags::CS8 | ControlFlags::CREAD | ControlFlags::HUPCL;
    t.local_flags |= LocalFlags::ECHO
        | LocalFlags::ECHOE
        | LocalFlags::ECHOK
        | LocalFlags::ECHOCTL
        | LocalFlags::ICANON
        | LocalFlags::ISIG
        | LocalFlags::IEXTEN;
    t.control_chars[SpecialCharacterIndices::VEOF as usize] = 4;
    t.control_chars[SpecialCharacterIndices::VINTR as usize] = 3;
    t.control_chars[SpecialCharacterIndices::VMIN as usize] = 1;
    t.control_chars[SpecialCharacterIndices::VTIME as usize] = 0;
    cfsetispeed(&mut t, nix::sys::termios::BaudRate::B38400).unwrap();
    cfsetospeed(&mut t, nix::sys::termios::BaudRate::B38400).unwrap();
    t
}

struct Harness {
    master: File,
    slave: OwnedFd,
    child: std::process::Child,
}

fn spawn_with(lade_bin: &Path, project: &Path, inject_cmd: &str) -> Harness {
    let size = Winsize {
        ws_row: 40,
        ws_col: 120,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let OpenptyResult { master, slave } = openpty(&size, None).unwrap();
    let t = cooked_termios();
    tcsetattr(slave.as_fd(), SetArg::TCSANOW, &t).unwrap();

    let slave_in: OwnedFd = slave.try_clone().unwrap();
    let slave_out: OwnedFd = slave.try_clone().unwrap();
    let slave_err: OwnedFd = slave.try_clone().unwrap();

    let child = Command::new(lade_bin)
        .args(["inject", inject_cmd])
        .current_dir(project)
        .env("LADE_SHELL", "bash")
        .env("HOME", project)
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .stdin(Stdio::from(slave_in))
        .stdout(Stdio::from(slave_out))
        .stderr(Stdio::from(slave_err))
        .spawn()
        .unwrap();

    Harness {
        master: File::from(master),
        slave,
        child,
    }
}

fn spawn(lade_bin: &Path, project: &Path) -> Harness {
    spawn_with(
        lade_bin,
        project,
        &format!("printf '{CHILD_READY}\\n'; sleep 2"),
    )
}

fn drive(lade_bin: &Path, project: &Path) -> Vec<u8> {
    let Harness {
        master,
        slave,
        mut child,
    } = spawn(lade_bin, project);
    drop(slave);

    let mut reader = master.try_clone().unwrap();
    let mut writer = master;

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut captured: Vec<u8> = Vec::new();
    let mut injected = false;
    let mut garbage_offset: Option<usize> = None;
    let deadline = Instant::now() + Duration::from_secs(10);

    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                captured.extend_from_slice(&chunk);
                if !injected && contains(&captured, CHILD_READY.as_bytes()) {
                    thread::sleep(Duration::from_millis(100));
                    garbage_offset = Some(captured.len());
                    writer
                        .write_all(OSC_RESPONSE)
                        .and_then(|_| writer.write_all(CPR_RESPONSE))
                        .and_then(|_| writer.flush())
                        .expect("inject write to master failed");
                    injected = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if injected && let Ok(Some(_)) = child.try_wait() {
            while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
                captured.extend_from_slice(&chunk);
            }
            break;
        }
    }

    let _ = child.kill();
    assert!(
        injected,
        "harness never saw child-ready marker; captured:\n{}",
        String::from_utf8_lossy(&captured)
    );
    captured[garbage_offset.unwrap()..].to_vec()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

// Minimal reader/writer loop that accumulates master output until either a
// deadline is hit or the child exits. Supports an optional "when you see
// `trigger`, write `response`" hook used by the interactive-read test.
fn run_until_exit(
    lade_bin: &Path,
    project: &Path,
    inject_cmd: &str,
    trigger: Option<(&[u8], &[u8])>,
) -> (Vec<u8>, std::process::ExitStatus) {
    let Harness {
        master,
        slave,
        mut child,
    } = spawn_with(lade_bin, project, inject_cmd);
    drop(slave);

    let mut reader = master.try_clone().unwrap();
    let mut writer = master;

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut captured: Vec<u8> = Vec::new();
    let mut triggered = trigger.is_none();
    let deadline = Instant::now() + Duration::from_secs(8);

    let status = loop {
        if let Ok(Some(s)) = child.try_wait() {
            while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
                captured.extend_from_slice(&chunk);
            }
            break s;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break child.wait().unwrap();
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                captured.extend_from_slice(&chunk);
                if let (false, Some((needle, response))) = (triggered, trigger)
                    && contains(&captured, needle)
                {
                    writer.write_all(response).unwrap();
                    writer.flush().unwrap();
                    triggered = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break child.wait().unwrap();
            }
        }
    };

    (captured, status)
}

fn write_redactor_yml(project: &Path, secret_value: &str) {
    std::fs::write(
        project.join("lade.yml"),
        format!("\"^(printf|bash|read)\":\n  LADE_HARNESS_SECRET: {secret_value}\n"),
    )
    .unwrap();
}

#[test]
fn stdin_escape_responses_do_not_echo_through_pty_path() {
    let dir = tempdir().unwrap();
    let project = dir.path();
    // 1-byte secret so the redactor's carry buffer (`max_pattern_len - 1`)
    // is zero — child output flushes immediately and the harness doesn't
    // need to wait for EOF to observe markers.
    write_redactor_yml(project, "x");

    let lade_bin = assert_cmd::cargo::cargo_bin("lade");
    let output = drive(&lade_bin, project);
    let text = String::from_utf8_lossy(&output);

    assert!(
        !contains(&output, OSC_MARKER),
        "OSC 11 response was echoed back through the PTY path; captured (lossy):\n{text}"
    );
    assert!(
        !contains(&output, CPR_MARKER),
        "CPR response was echoed back through the PTY path; captured (lossy):\n{text}"
    );
}

// `RawStdinGuard` flips the real TTY into raw mode for the duration of
// `run_pty`. On drop it must restore the original termios; otherwise the
// user's shell is left with ECHO/ICANON off after lade exits — equivalent
// to running `stty raw`, which is a hard-to-diagnose regression.
#[test]
fn termios_is_restored_after_lade_exits() {
    let dir = tempdir().unwrap();
    let project = dir.path();
    write_redactor_yml(project, "x");

    let lade_bin = assert_cmd::cargo::cargo_bin("lade");
    let Harness {
        master,
        slave,
        mut child,
    } = spawn_with(lade_bin.as_path(), project, "true");
    let before = nix::sys::termios::tcgetattr(slave.as_fd()).unwrap();

    let status = child.wait().unwrap();
    assert!(status.success(), "lade exited with {status:?}");

    let after = nix::sys::termios::tcgetattr(slave.as_fd()).unwrap();
    drop(master);
    drop(slave);

    assert_eq!(
        before.local_flags, after.local_flags,
        "local_flags changed: before={:?} after={:?}",
        before.local_flags, after.local_flags
    );
    assert_eq!(before.input_flags, after.input_flags, "input_flags changed");
    assert_eq!(
        before.output_flags, after.output_flags,
        "output_flags changed"
    );
    assert_eq!(
        before.control_flags, after.control_flags,
        "control_flags changed"
    );
}

// The PTY path puts stdin in raw mode; programs like bash's `read` and
// tofu's confirmation prompt reinstate canonical mode themselves while
// reading a line. This test pins the contract that a bash `read -p` round-
// trips a line written from the terminal side — if a future change
// broadened raw mode past program-managed termios restoration, `read`
// would block forever and this test would hit the deadline.
#[test]
fn interactive_line_read_round_trips_through_pty_path() {
    let dir = tempdir().unwrap();
    let project = dir.path();
    write_redactor_yml(project, "x");

    let lade_bin = assert_cmd::cargo::cargo_bin("lade");
    let (captured, status) = run_until_exit(
        &lade_bin,
        project,
        "printf '> '; read -r v; printf 'got=%s\\n' \"$v\"",
        Some((b"> ", b"world\n")),
    );
    let text = String::from_utf8_lossy(&captured);

    assert!(
        status.success(),
        "lade exit status={status:?} captured:\n{text}"
    );
    assert!(
        contains(&captured, b"got=world"),
        "expected `got=world` in captured output; got (lossy):\n{text}"
    );
}

// Defence in depth: verify the Aho-Corasick redactor is actually spliced
// into the PTY stdout path. The selection logic is unit-tested, but
// nothing would catch a `run_pty` refactor that accidentally bypassed
// `redactor.stream` (e.g. by wiring the child's stdout straight to lade's
// stdout).
#[test]
fn secret_is_redacted_on_pty_path() {
    let dir = tempdir().unwrap();
    let project = dir.path();
    // Multi-byte secret exercises the Aho-Corasick match; trailing padding
    // in the command ensures the bytes clear the redactor's carry buffer
    // before EOF.
    std::fs::write(
        project.join("lade.yml"),
        "\"^printf\":\n  LADE_HARNESS_SECRET: SUPERSECRET\n",
    )
    .unwrap();

    let lade_bin = assert_cmd::cargo::cargo_bin("lade");
    let (captured, status) = run_until_exit(
        &lade_bin,
        project,
        "printf 'before SUPERSECRET after %s\\n' ----------",
        None,
    );
    let text = String::from_utf8_lossy(&captured);

    assert!(status.success(), "lade exit status={status:?}");
    assert!(
        !contains(&captured, b"SUPERSECRET"),
        "raw secret leaked through PTY path; captured (lossy):\n{text}"
    );
    assert!(
        contains(&captured, b"${LADE_HARNESS_SECRET:-REDACTED}"),
        "expected redaction token in captured output; got (lossy):\n{text}"
    );
}
