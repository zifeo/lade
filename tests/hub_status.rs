//! Commands that have nothing for the hub, or only read or stop it, never
//! start one, and `lade status` shows the memory guard.

mod common;

use std::path::Path;

fn hub_sockets(home: &Path) -> Vec<String> {
    std::fs::read_dir(home.join("lade-cache"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("hub-") && name.ends_with(".sock"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_plain_command_starts_no_hub() {
    let home = tempfile::tempdir().unwrap();
    let raw = tempfile::tempdir().unwrap();
    std::fs::write(
        raw.path().join("lade.yml"),
        "\"^echo\":\n  KEY: plain-value\n",
    )
    .unwrap();
    let none = tempfile::tempdir().unwrap();
    for dir in [raw.path(), none.path()] {
        common::lade(home.path())
            .current_dir(dir)
            .args(["set", "echo hi"])
            .assert()
            .success();
        common::lade(home.path())
            .current_dir(dir)
            .args(["--", "echo", "hi"])
            .assert()
            .success();
    }
    assert_eq!(hub_sockets(home.path()), Vec::<String>::new());
}

#[test]
fn a_pretool_wrap_that_only_cancels_keys_starts_no_hub() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        root.path().join("lade.yml"),
        "\"^printenv\":\n  KEY: parent-value\n",
    )
    .unwrap();
    std::fs::write(child.join("lade.yml"), "\"^printenv\":\n  KEY: ~\n").unwrap();
    common::lade(home.path())
        .current_dir(&child)
        .args(["--pretool", "inject", "--no-mask", "printenv KEY"])
        .assert()
        .failure()
        .stdout("");
    assert_eq!(hub_sockets(home.path()), Vec::<String>::new());
}

#[test]
fn a_preexec_set_that_only_cancels_keys_starts_no_hub() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        root.path().join("lade.yml"),
        "\"^printenv\":\n  KEY: parent-value\n",
    )
    .unwrap();
    std::fs::write(child.join("lade.yml"), "\"^printenv\":\n  KEY: ~\n").unwrap();
    // A human's stderr on a terminal: `lade set` would ask for the notice
    // slot, which starts a hub.
    let pty = nix::pty::openpty(None, None).unwrap();
    let mut bash = std::process::Command::new("bash");
    bash.args(["-c", "\"$0\" \"$@\" && true"])
        .arg(assert_cmd::cargo::cargo_bin("lade"))
        .args(["set", "printenv KEY"]);
    let out = common::with_lade_env(bash, home.path())
        .current_dir(&child)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::from(pty.slave))
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();
    drop(pty.master);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("LADE_T="), "{stdout}");
    assert_eq!(hub_sockets(home.path()), Vec::<String>::new());

    // A hub already up holds the pending ticket a withheld disclaimer left in
    // `LADE_T`: the cancel-only line replaces it without a word to the hub.
    let secret = tempfile::tempdir().unwrap();
    let value = secret.path().join("value.json");
    std::fs::write(&value, r#"{"token":"v"}"#).unwrap();
    std::fs::write(
        secret.path().join("lade.yml"),
        format!(
            "\"^printenv\":\n  TOKEN: file://{}?query=.token\n",
            value.display()
        ),
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(secret.path())
        .args(["set", "printenv TOKEN"])
        .assert()
        .success();
    let gate = tempfile::tempdir().unwrap();
    std::fs::write(
        gate.path().join("lade.yml"),
        "\"^deploy\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(gate.path())
        .args(["set", "deploy"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let held = common::extract_lade_t(&String::from_utf8_lossy(&out.stdout)).expect("a ticket");
    let tickets = || {
        let out = common::lade(home.path())
            .current_dir(&child)
            .args(["ttl", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice::<serde_json::Value>(&out).unwrap()["tickets"].clone()
    };
    assert_eq!(tickets(), 2);
    common::lade(home.path())
        .current_dir(&child)
        .env("LADE_T", &held)
        .args(["set", "printenv KEY"])
        .assert()
        .success();
    let after = tickets();
    common::lade(home.path())
        .current_dir(&child)
        .args(["ttl", "forget"])
        .assert()
        .success();
    assert_eq!(after, 2);
}

#[test]
fn commands_that_read_or_stop_the_hub_start_none() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["status"][..],
        &["status", "--json"],
        &["ttl"],
        &["ttl", "forget"],
        &["log", "prune", "--hub"],
    ] {
        common::lade(home.path())
            .current_dir(dir.path())
            .args(args)
            .assert()
            .success();
        assert_eq!(hub_sockets(home.path()), Vec::<String>::new(), "{args:?}");
    }
}

#[test]
fn status_shows_the_memory_guard() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["status"])
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"(?m)^ *guard +cache (on|off): ").unwrap());
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let state = report["hub"]["memory"]["state"]
        .as_str()
        .unwrap_or_default();
    assert!(
        ["hardened_runtime", "not_dumpable", "debug_build", "off"].contains(&state),
        "{}",
        report["hub"]
    );
}
