mod common;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_inject_stdin_escape_responses_do_not_leak_to_stdout() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  SECRET: stdincheck42\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "echo hello"])
        .write_stdin("\x1b]11;rgb:1f1f/2424/2828\x1b\\\x1b[37;1R")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("hello"), "expected child output: {stdout}");
    assert!(
        !stdout.contains("\x1b]11;"),
        "OSC 11 response leaked into stdout: {stdout:?}"
    );
    assert!(
        !stdout.contains("\x1b[37;1R"),
        "CPR response leaked into stdout: {stdout:?}"
    );
}

#[test]
fn test_inject_exit_code_propagation() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "exit 7"])
        .assert()
        .code(7);
}

#[test]
fn test_alias_exit_code_propagation() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["exit", "7"])
        .assert()
        .code(7);
}

#[test]
fn test_alias_with_separator() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  SECRET: separator_secret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["--", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("separator_secret"));
}

#[test]
fn test_inject_with_separator() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--", "echo", "separator"])
        .assert()
        .success()
        .stdout("separator\n");
}

#[test]
fn test_separator_at_end_is_forwarded_to_child() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--", "echo", "separator", "--"])
        .assert()
        .success()
        .stdout("separator --\n");
}

#[test]
fn test_inject_requires_command() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("inject")
        .assert()
        .failure()
        .stderr(predicates::str::contains("A command is required"));
}

#[test]
fn test_inject_separator_requires_command() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("A command is required"));
}
