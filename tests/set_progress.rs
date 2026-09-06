mod common;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_set_overlay_shows_overridden_progress() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        ".:\n  TOKEN: a\n\"echo\":\n  TOKEN: b\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "echo hi"])
        .assert()
        .success()
        .stderr(predicates::str::contains("TOKEN (overridden)"));
}

#[test]
fn test_set_git_cancel_shows_cancelled_progress() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        ".:\n  SSH_AUTH_SOCK: \"\"\n\"^git \":\n  SSH_AUTH_SOCK: ~\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "git status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export SSH_AUTH_SOCK").not())
        .stderr(predicates::str::contains("SSH_AUTH_SOCK (cancelled)"));
}

#[test]
fn test_set_silence_hides_hydration_progress() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo\":\n  \".\":\n    silence: true\n  KEY: val\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "echo hi"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export KEY="))
        .stderr(predicates::str::contains("KEY").not());
}

#[test]
fn test_set_without_silence_shows_hydration_progress() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(dir.path().join("lade.yml"), "\"echo\":\n  KEY: val\n").unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "echo hi"])
        .assert()
        .success()
        .stderr(predicates::str::contains("Raw: KEY"));
}

#[test]
#[cfg(unix)]
fn test_inject_with_fake_vault_cli() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let fake_bin = tempdir().unwrap();
    common::fake_cli(
        &fake_bin,
        "vault",
        r#"echo '{"data":{"data":{"password":"vault_injected"}}}'"#,
    );
    fs::write(
        dir.path().join("lade.yml"),
        "\"vault.*\":\n  PASSWORD: \"vault://localhost/secret/myapp/password\"\n",
    )
    .unwrap();
    let new_path = format!(
        "{}:{}",
        fake_bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    common::lade(home.path())
        .current_dir(dir.path())
        .env("PATH", &new_path)
        .args(["set", "vault cmd"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "export PASSWORD='vault_injected'",
        ));
}

#[test]
fn test_set_skips_agent_when_rules() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  \".\":\n    when: agent\n  SECRET: agentsecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "mycmd"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export SECRET").not());
}

#[test]
fn test_set_fish_still_evals_in_the_interactive_shell() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_SHELL", "fish")
        .args(["set", "mycmd"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(
        stdout.contains("set --global --export SECRET 'mysecret'"),
        "preexec must keep fish set() syntax: {stdout}"
    );
    assert!(
        !stdout.contains("--no-config"),
        "preexec must not switch the interactive shell to --no-config: {stdout}"
    );
}
