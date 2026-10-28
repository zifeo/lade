#![cfg(unix)]
//! Lade runs a line in the shell that started it: the harness's shell
//! for a pre-tool rewrite, whatever terminal the harness came from.

mod common;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::path::Path;
use std::process::Command as StdCommand;
use tempfile::tempdir;

/// Fish rejects `${`, so this line also fails if fish runs it.
const IDENTITY: &str = r#"echo "${BASH_VERSION:+bash}${ZSH_VERSION:+zsh}""#;
const POSIX: &[(&str, &str)] = &[
    ("for i in 1 2; do echo $i; done", "1\n2\n"),
    ("x=1 && echo $x", "1\n"),
];
const GLOB: &str = "printf '%s\\n' --include=*.rs";

fn has(shell: &str) -> bool {
    StdCommand::new(shell)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The rewrite the hook emits when it cannot write a ticket.
fn idless(command: &str) -> String {
    format!(
        "'{}' --pretool -- '{}' && true",
        assert_cmd::cargo::cargo_bin("lade").display(),
        command.replace('\'', "'\\''")
    )
}

fn run(home: &Path, dir: &Path, shell: &[&str], line: &str) -> assert_cmd::assert::Assert {
    common::shell_line(home, shell, line)
        .current_dir(dir)
        .assert()
}

#[test]
fn pretool_runs_in_bash_parent() {
    let (home, dir) = (tempdir().unwrap(), tempdir().unwrap());
    let (home, dir) = (home.path(), dir.path());
    for (command, out) in POSIX {
        run(home, dir, &["bash"], &idless(command))
            .success()
            .stdout(*out);
    }
    run(home, dir, &["bash"], &idless(GLOB))
        .success()
        .stdout("--include=*.rs\n");
    run(home, dir, &["bash"], &idless(IDENTITY))
        .success()
        .stdout("bash\n");
}

#[test]
fn pretool_runs_in_zsh_parent() {
    if !has("zsh") {
        eprintln!("zsh not installed: skipped");
        return;
    }
    let (home, dir) = (tempdir().unwrap(), tempdir().unwrap());
    let (home, dir) = (home.path(), dir.path());
    let zsh = ["zsh", "-f"];
    for (command, out) in POSIX {
        run(home, dir, &zsh, &idless(command))
            .success()
            .stdout(*out);
    }
    // zsh's own NOMATCH error, as when the harness runs the line in zsh.
    run(home, dir, &zsh, &idless(GLOB))
        .failure()
        .stderr(contains("no matches found: --include=*.rs"))
        .stderr(contains("No matches for wildcard").not());
    run(home, dir, &zsh, &idless(IDENTITY))
        .success()
        .stdout("zsh\n");
}

#[test]
fn pretool_runs_in_fish_parent() {
    if !has("fish") {
        eprintln!("fish not installed: skipped");
        return;
    }
    let (home, dir) = (tempdir().unwrap(), tempdir().unwrap());
    run(
        home.path(),
        dir.path(),
        &["fish", "--no-config"],
        &idless("set x 1; echo $x"),
    )
    .success()
    .stdout("1\n");
}

#[test]
fn pretool_without_a_shell_parent_fails() {
    let (home, dir) = (tempdir().unwrap(), tempdir().unwrap());
    let mut cmd = common::lade_std(home.path());
    cmd.current_dir(dir.path())
        .args(["--pretool", "--", "echo hi"]);
    assert_cmd::Command::from_std(cmd)
        .assert()
        .failure()
        .stdout("")
        .stderr(contains("shell that started Lade"));
}

#[test]
fn a_direct_inject_runs_in_its_parent_shell_too() {
    let (home, dir) = (tempdir().unwrap(), tempdir().unwrap());
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--no-mask", "x=1 && echo $x"])
        .assert()
        .success()
        .stdout("1\n");
}

#[test]
fn with_no_shell_above_a_line_runs_in_sh() {
    let (home, dir) = (tempdir().unwrap(), tempdir().unwrap());
    let mut cmd = common::lade_std(home.path());
    cmd.current_dir(dir.path())
        .args(["inject", "--no-mask", "x=1 && echo $x"]);
    assert_cmd::Command::from_std(cmd)
        .assert()
        .success()
        .stdout("1\n");
}

/// The rewrite names a bare `lade` when one is on PATH: make it this build.
fn path_with_built_lade() -> String {
    let bin = assert_cmd::cargo::cargo_bin("lade");
    format!(
        "{}:{}",
        bin.parent().unwrap().display(),
        std::env::var("PATH").unwrap()
    )
}

fn hook_rewrite(home: &Path, dir: &Path, command: &str) -> String {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "hook_event_name": "PreToolUse",
        "cwd": dir,
    });
    let out = common::lade(home)
        .current_dir(dir)
        .env("PATH", path_with_built_lade())
        .arg("hook")
        .write_stdin(payload.to_string())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
    parsed["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap_or_else(|| panic!("rewrite in {parsed}"))
        .to_string()
}

#[test]
fn hook_rewrite_keeps_the_harness_shell_as_parent() {
    let (home, dir) = (tempdir().unwrap(), tempdir().unwrap());
    let (home, dir) = (home.path(), dir.path());
    common::write_yml_raw(dir, "\"^for \":\n  UNUSED: unused_value_0000000001\n");
    let command = POSIX[0].0;
    let mut shells: Vec<&[&str]> = vec![&["bash"]];
    if has("zsh") {
        shells.push(&["zsh", "-f"]);
    } else {
        eprintln!("zsh not installed: zsh case skipped");
    }
    for shell in shells {
        let line = hook_rewrite(home, dir, command);
        assert!(line.contains("--pretool="), "{line}");
        // A bash or zsh `-c` with one command execs it, so Lade would
        // lose the shell it must run the command in.
        common::shell_line(home, shell, &line)
            .current_dir(dir)
            .env("PATH", path_with_built_lade())
            .assert()
            .success()
            .stdout(POSIX[0].1);
    }
}
