//! Who reaches which hub, through real `lade` binaries: a client of another
//! executable is denied, two files of one build get two hubs, a binary not
//! named `lade` starts none, and concurrent starts leave one.

mod common;

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn hub_sockets(home: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(home.join("lade-cache"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("hub-") && name.ends_with(".sock"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `exe` with the test Lade env. A debug client would start the hub from
/// `CARGO_BIN_EXE_lade` if the environment held it.
///
/// A copy just written can be busy for an instant on Linux: another test
/// thread that forks meanwhile holds its write fd until that child execs.
fn run(exe: &Path, home: &Path, dir: &Path, args: &[&str]) -> std::process::Output {
    for _ in 0..50 {
        let mut cmd = common::with_lade_env(Command::new(exe), home);
        match cmd
            .env_remove("CARGO_BIN_EXE_lade")
            .current_dir(dir)
            .args(args)
            .output()
        {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(20));
            }
            out => return out.unwrap(),
        }
    }
    panic!("{} stayed busy", exe.display())
}

fn hub_json(exe: &Path, home: &Path, dir: &Path) -> serde_json::Value {
    let out = run(exe, home, dir, &["status", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    report["hub"].clone()
}

fn stop_hub(exe: &Path, home: &Path, dir: &Path) {
    let out = run(exe, home, dir, &["log", "prune", "--hub"]);
    assert!(out.status.success(), "{out:?}");
}

/// A copy of the `lade` binary, a file of its own.
fn copy_of_lade(dir: &Path, name: &str) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let copy = bin.join(name);
    std::fs::copy(env!("CARGO_BIN_EXE_lade"), &copy).unwrap();
    copy
}

fn lade_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lade"))
}

#[test]
fn a_client_of_another_binary_is_denied() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let lade = lade_exe();
    let out = run(&lade, home.path(), dir.path(), &["ttl", "set", "1h"]);
    assert!(out.status.success(), "{out:?}");
    let sockets = hub_sockets(home.path());
    assert_eq!(sockets.len(), 1, "{sockets:?}");

    // A well-formed frame whose body is no request. The hub's own binary
    // would get no reply at all. This test binary gets `Err` 4 before the
    // hub reads a byte.
    let mut stream = UnixStream::connect(&sockets[0]).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(b"LDH1\x04\0\0\0junk").unwrap();
    // One frame, not to the end: on Linux the hub closing with the junk
    // unread resets the socket after its reply, instead of an EOF.
    let mut reply = vec![0u8; 8];
    stream.read_exact(&mut reply).unwrap();
    let n = u32::from_le_bytes(reply[4..8].try_into().unwrap()) as usize;
    let mut body = vec![0u8; n];
    stream.read_exact(&mut body).unwrap();
    let up = hub_json(&lade, home.path(), dir.path());
    stop_hub(&lade, home.path(), dir.path());

    assert!(reply.starts_with(b"LDH1"), "{reply:?}");
    assert_eq!(up["state"], "up", "{up}");
}

#[test]
fn two_files_of_one_build_get_two_hubs() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let lade = lade_exe();
    let copy = copy_of_lade(home.path(), "lade");
    for exe in [&lade, &copy] {
        let out = run(exe, home.path(), dir.path(), &["ttl", "set", "1h"]);
        assert!(out.status.success(), "{out:?}");
    }
    let sockets = hub_sockets(home.path());
    let ours = hub_json(&lade, home.path(), dir.path());
    let theirs = hub_json(&copy, home.path(), dir.path());
    stop_hub(&lade, home.path(), dir.path());
    stop_hub(&copy, home.path(), dir.path());

    assert_eq!(sockets.len(), 2, "{sockets:?}");
    assert_eq!(ours["state"], "up", "{ours}");
    assert_eq!(theirs["state"], "up", "{theirs}");
    assert_ne!(ours["pid"], theirs["pid"]);
}

#[test]
fn a_binary_not_named_lade_starts_no_hub() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let other = copy_of_lade(home.path(), "lade-other");
    let out = run(&other, home.path(), dir.path(), &["ttl", "set", "1h"]);
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("hub: down"),
        "{out:?}"
    );
    assert_eq!(hub_sockets(home.path()), Vec::<PathBuf>::new());
}

#[test]
fn concurrent_starts_leave_one_hub() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // A file of its own, so no other test's hub matches its command line.
    let lade = copy_of_lade(home.path(), "lade");
    let starts = (0..10)
        .map(|_| {
            let mut cmd = common::with_lade_env(Command::new(&lade), home.path());
            cmd.env_remove("CARGO_BIN_EXE_lade")
                .current_dir(dir.path())
                .args(["ttl", "set", "1h"])
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let statuses = starts
        .into_iter()
        .map(|mut child| child.wait().unwrap())
        .collect::<Vec<_>>();
    let sockets = hub_sockets(home.path());
    let pattern = format!("{} __hub", lade.display());
    let hubs = Command::new("pgrep")
        .args(["-f", &pattern])
        .output()
        .unwrap();
    stop_hub(&lade, home.path(), dir.path());

    assert!(
        statuses.iter().all(|status| status.success()),
        "{statuses:?}"
    );
    assert_eq!(sockets.len(), 1, "{sockets:?}");
    let pids = String::from_utf8_lossy(&hubs.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(pids, 1, "{hubs:?}");
}
