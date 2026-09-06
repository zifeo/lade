mod common;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_inject_agent_when_uses_env_signal() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  \".\":\n    when: agent\n  SECRET: agentsecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("CURSOR_AGENT", "1")
        .args(["inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("agentsecret"));
    common::lade(home.path())
        .current_dir(dir.path())
        .env("CURSOR_VERSION", "1.0")
        .args(["inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("agentsecret").not());
}

#[test]
fn test_inject_agent_when_requires_pretool_stamp() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  \".\":\n    when: agent\n  SECRET: agentsecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("agentsecret").not());
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["--pretool", "inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("agentsecret"));
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_VIA", "pretool")
        .args(["inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("agentsecret").not());
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_VIA", "preexec")
        .args(["--pretool", "inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("agentsecret"));
}

#[test]
fn test_inject_pretool_does_not_stamp_via_on_child() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["--pretool", "inject", "--no-mask", "echo", "via=$LADE_VIA"])
        .assert()
        .success()
        .stdout(predicates::str::contains("via=pretool").not())
        .stdout(predicates::str::contains("via=preexec").not());
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["--pretool", "echo", "via=$LADE_VIA"])
        .assert()
        .success()
        .stdout(predicates::str::contains("via=pretool").not());
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_VIA", "pretool")
        .args(["inject", "--no-mask", "echo", "via=$LADE_VIA"])
        .assert()
        .success()
        .stdout(predicates::str::contains("via=pretool").not());
}

#[test]
fn test_inject_without_stamp_strips_via_from_child() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--no-mask", "echo", "via=$LADE_VIA"])
        .assert()
        .success()
        .stdout(predicates::str::contains("via=pretool").not())
        .stdout(predicates::str::contains("via=preexec").not());
}
