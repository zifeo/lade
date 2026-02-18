mod common;

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
        .stdout(predicates::str::contains("export SECRET='mysecret'"));
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
        .stdout(predicates::str::contains("unset -v SECRET"));
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
    let lade_yml = format!(
        "\"cmd\":\n  VALUE: \"file://{}?query=.api_key\"\n",
        source.display()
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
fn test_inject_raw_value_reaches_child_process() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  SECRET: injected_secret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("injected_secret"));
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
