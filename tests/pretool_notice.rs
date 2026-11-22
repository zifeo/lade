#![cfg(unix)]
//! A pre-tool wrap that uses the hub gets the hourly notice slot, and its
//! box asks the agent to tell the human, once an hour.

mod common;
use std::fs;
use tempfile::tempdir;

#[test]
fn a_pretool_wrap_relays_the_hourly_notice_once() {
    let (home, root) = (tempdir().unwrap(), tempdir().unwrap());
    let secret = root.path().join("token.json");
    fs::write(&secret, r#"{"v":"t"}"#).unwrap();
    fs::write(
        root.path().join("lade.yml"),
        format!(
            "\"^printenv\":\n  TOKEN: \"file://{}?query=.v\"\n",
            secret.display()
        ),
    )
    .unwrap();
    // A hook file Lade cannot read: it never rewrites it, so the notice
    // names it on every check.
    let cursor = home.path().join(".cursor");
    fs::create_dir_all(&cursor).unwrap();
    fs::write(cursor.join("hooks.json"), "{").unwrap();

    let wrap = || {
        let out = common::lade(home.path())
            .current_dir(root.path())
            .args(["--pretool", "inject", "--no-mask", "printenv TOKEN"])
            .assert()
            .success()
            .get_output()
            .clone();
        String::from_utf8(out.stderr).unwrap()
    };
    let first = wrap();
    let second = wrap();
    common::lade(home.path())
        .current_dir(root.path())
        .args(["log", "prune", "--hub"])
        .assert()
        .success();

    assert_eq!(first.matches("Tell the human:").count(), 1, "{first}");
    let line = first
        .lines()
        .find(|line| line.contains("Tell the human:"))
        .unwrap();
    assert!(line.starts_with("lade: "), "{line}");
    assert!(line.contains("hooks.json"), "{line}");
    assert!(!second.contains("Tell the human:"), "{second}");
}
