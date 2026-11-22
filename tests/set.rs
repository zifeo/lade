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
            predicates::str::contains("Raw: SECRET")
                .not()
                .and(predicates::str::contains("not a vault secret").not()),
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
    assert!(
        !stdout.contains("LADE_EVENT_ID"),
        "unexpected exports: {stdout}"
    );
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
    let id = common::extract_lade_t(&set_stdout).expect("LADE_T export");
    let unset_stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .env("LADE_RESTORE", restore)
            .env("LADE_T", id)
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
fn test_unset_corrupt_restore_unsets_the_tunnels_the_ticket_names() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    let tickets = home.path().join("lade-cache").join("tickets");
    fs::create_dir_all(&tickets).unwrap();
    let ticket = serde_json::json!({
        "command": "mycmd",
        "cwd": dir.path(),
        "via": "preexec",
        "audience": "human",
        "actor": null,
        "log": false,
        "disclaimers": [],
        "secrets": [{"key": "SECRET", "source": "mysecret", "private": false, "cwd": dir.path()}],
        "network": [
            {"key": "DB_PORT", "uri": "ssh://jump.example.com/db.internal/5432"},
            {"key": "6543", "uri": "ssh://jump.example.com/db.internal/5432"}
        ],
        "matches": []
    });
    fs::write(tickets.join("Tk1x.json"), ticket.to_string()).unwrap();
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_RESTORE", "v1:bad")
        .env("LADE_T", "Tk1x")
        .args(["unset", "mycmd"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&stdout);
    assert!(stdout.contains("unset -v SECRET"), "{stdout}");
    assert!(stdout.contains("unset -v DB_PORT"), "{stdout}");
    assert!(!stdout.contains("6543"), "{stdout}");
}

#[test]
fn test_unset_removes_the_file_output() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  .:\n    file: out.json\n  KEY: value\n",
    )
    .unwrap();
    let set_stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .args(["set", "mycmd"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .into_owned();
    let out = dir.path().join("out.json");
    assert!(out.exists(), "set writes the file: {set_stdout}");
    let marker = "export LADE_RESTORE='";
    let start = set_stdout.find(marker).expect("LADE_RESTORE export");
    let rest = &set_stdout[start + marker.len()..];
    let restore = &rest[..rest.find('\'').expect("restore payload end")];
    let id = common::extract_lade_t(&set_stdout).expect("LADE_T export");
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_RESTORE", restore)
        .env("LADE_T", id)
        .args(["unset", "mycmd"])
        .assert()
        .success();
    assert!(!out.exists());
}

#[test]
fn unset_restores_the_shell_when_lade_yaml_broke_during_the_command() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  .:\n    file: out.json\n  FKEY: x\n\"^mycmd\":\n  EXISTING: after\n",
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
    let out = dir.path().join("out.json");
    assert!(out.exists(), "{set_stdout}");
    fs::write(dir.path().join("lade.yml"), "\"mycmd\": [unclosed\n").unwrap();
    let marker = "export LADE_RESTORE='";
    let start = set_stdout.find(marker).expect("LADE_RESTORE export");
    let rest = &set_stdout[start + marker.len()..];
    let restore = &rest[..rest.find('\'').expect("restore payload end")];
    let id = common::extract_lade_t(&set_stdout).expect("LADE_T export");
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_RESTORE", restore)
        .env("LADE_T", id)
        .args(["unset", "mycmd"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&stdout);
    assert!(stdout.contains("export EXISTING='before'"), "{stdout}");
    assert!(!out.exists());
}

#[test]
fn unset_restores_the_shell_before_it_fails_on_a_file_it_cannot_remove() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  .:\n    file: out.json\n  KEY: value\n",
    )
    .unwrap();
    let set_stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .args(["set", "mycmd"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .into_owned();
    let out = dir.path().join("out.json");
    fs::remove_file(&out).unwrap();
    fs::create_dir_all(out.join("held")).unwrap();
    let marker = "export LADE_RESTORE='";
    let start = set_stdout.find(marker).expect("LADE_RESTORE export");
    let rest = &set_stdout[start + marker.len()..];
    let restore = &rest[..rest.find('\'').expect("restore payload end")];
    let id = common::extract_lade_t(&set_stdout).expect("LADE_T export");
    let assert = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_RESTORE", restore)
        .env("LADE_T", id)
        .args(["unset", "mycmd"])
        .assert()
        .failure()
        .code(1);
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("unset -v LADE_RESTORE"), "{stdout}");
    assert!(stderr.contains("out.json"), "{stderr}");
}

#[test]
fn set_with_no_shell_to_cancel_in_fails_before_the_yaml() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(dir.path().join("lade.yml"), "\"cmd\": [unclosed\n").unwrap();
    assert_cmd::Command::from_std(common::lade_std(home.path()))
        .current_dir(dir.path())
        .args(["set", "cmd"])
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(predicates::str::contains("Unsupported shell"))
        .stderr(predicates::str::contains("lade.yaml").not());
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
