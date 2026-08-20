mod common;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_set_raw_values() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "mycmd"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export SECRET='mysecret'"))
        .stderr(
            predicates::str::contains("Lade connecting: Raw: SECRET")
                .and(predicates::str::contains("Lade connected: Raw: SECRET")),
        );
}

#[test]
fn test_set_multiple_secrets() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  KEY1: val1\n  KEY2: val2\n",
    )
    .unwrap();
    let output = common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "mycmd"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains("export KEY1='val1'"), "stdout: {stdout}");
    assert!(stdout.contains("export KEY2='val2'"), "stdout: {stdout}");
}

#[test]
fn test_unset_keys() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["unset", "mycmd"])
        .assert()
        .success()
        .stdout(
            predicates::str::contains("unset -v LADE_RESTORE")
                .and(predicates::str::contains("unset -v SECRET").not()),
        );
}

#[test]
fn test_set_no_lade_yml_exits_cleanly() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "mycmd"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(!stdout.contains("export"), "unexpected exports: {stdout}");
}

#[test]
fn test_set_malformed_lade_yml_fails() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  \".\": \"old_string_format\"\n  KEY: val\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "cmd"])
        .assert()
        .failure();
}

#[test]
fn test_set_with_file_provider() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let source = dir.path().join("source.json");
    fs::write(&source, r#"{"api_key":"filevalue123"}"#).unwrap();
    let source_url_path = source.to_str().unwrap().replace('\\', "/");
    let lade_yml = format!(
        "\"cmd\":\n  VALUE: \"file://{}?query=.api_key\"\n",
        source_url_path
    );
    fs::write(dir.path().join("lade.yml"), &lade_yml).unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "cmd"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export VALUE='filevalue123'"));
}

#[test]
fn test_unset_restores_previous_env() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  EXISTING: after\n  SECRET: mysecret\n",
    )
    .unwrap();
    let set_stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .env("EXISTING", "before")
            .args(["set", "mycmd"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .into_owned();
    let marker = "export LADE_RESTORE='";
    let start = set_stdout.find(marker).expect("LADE_RESTORE export");
    let rest = &set_stdout[start + marker.len()..];
    let end = rest.find('\'').expect("restore payload end");
    let restore = &rest[..end];
    let unset_stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .env("LADE_RESTORE", restore)
            .args(["unset", "mycmd"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .into_owned();
    assert!(
        unset_stdout.contains("export EXISTING='before'"),
        "stdout: {unset_stdout}"
    );
    assert!(
        unset_stdout.contains("unset -v SECRET"),
        "stdout: {unset_stdout}"
    );
    assert!(
        unset_stdout.contains("unset -v LADE_RESTORE"),
        "stdout: {unset_stdout}"
    );
}

#[test]
fn test_unset_corrupt_restore_fails() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_RESTORE", "v1:bad")
        .args(["unset", "mycmd"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicates::str::contains("export").not())
        .stderr(predicates::str::contains(
            "The previous environment snapshot is corrupted. Re-run the command.",
        ));
}

#[test]
fn test_set_empty_string_exports_empty() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(dir.path().join("lade.yml"), ".:\n  EMPTY: \"\"\n").unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "echo hi"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export EMPTY=''"));
}

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
