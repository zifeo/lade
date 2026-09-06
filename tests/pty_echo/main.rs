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

mod harness;

use harness::{
    CPR_MARKER, Harness, OSC_MARKER, contains, drive, run_until_exit, spawn_with,
    write_redactor_yml,
};
use std::os::fd::AsFd;
use tempfile::tempdir;

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
    let source = project.join("source.json");
    std::fs::write(&source, r#"{"token":"SUPERSECRET"}"#).unwrap();
    let source_url_path = source.to_str().unwrap().replace('\\', "/");
    std::fs::write(
        project.join("lade.yml"),
        format!(
            "\"^printf\":\n  LADE_HARNESS_SECRET: \"file://{}?query=.token\"\n",
            source_url_path
        ),
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
