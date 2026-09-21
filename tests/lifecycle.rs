mod common;

use std::fs;

use tempfile::tempdir;

fn git_repo(dir: &std::path::Path) {
    common::init_git(dir);
}

#[test]
fn setup_writes_project_pretool_hook() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["setup", "--harness", "cursor"])
        .assert()
        .success();

    let hook = dir.path().join(".cursor").join("hooks.json");
    let body = fs::read_to_string(&hook).unwrap();
    assert!(
        body.contains("lade hook --harness cursor"),
        "project hook should call pretool handler: {body}"
    );

    let pretool = common::lade(home.path())
        .current_dir(dir.path())
        .args(["status", "--json"])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&pretool.get_output().stdout).unwrap();
    assert!(
        value["hooks"]["pretool"]["cursor"]["project"]["installed"]
            .as_bool()
            .unwrap()
    );
    assert!(
        value["hooks"]["pretool"]["cursor"]["project"]["current"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn teardown_removes_project_pretool_hook() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["setup", "--harness", "cursor"])
        .assert()
        .success();
    assert!(dir.path().join(".cursor").join("hooks.json").is_file());

    common::lade(home.path())
        .current_dir(dir.path())
        .arg("teardown")
        .assert()
        .success();

    let hook = dir.path().join(".cursor").join("hooks.json");
    if hook.is_file() {
        let body = fs::read_to_string(&hook).unwrap();
        assert!(
            !body.contains("lade hook --harness cursor"),
            "teardown should remove the pretool hook: {body}"
        );
    }
}

#[test]
fn add_writes_binding_and_remove_drops_it() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");

    common::lade(home.path())
        .current_dir(dir.path())
        .args([
            "add",
            "secret",
            "--rule",
            "^deploy",
            "--key",
            "TOKEN",
            "--uri",
            "raw://from-add",
        ])
        .assert()
        .success();

    let yaml = fs::read_to_string(dir.path().join("lade.yml")).unwrap();
    assert!(yaml.contains("^deploy"));
    assert!(yaml.contains("TOKEN"));
    assert!(yaml.contains("from-add"));

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["remove", "secret", "--rule", "^deploy", "--key", "TOKEN"])
        .assert()
        .success();

    let yaml = fs::read_to_string(dir.path().join("lade.yml")).unwrap();
    assert!(!yaml.contains("^deploy"));
    assert!(!yaml.contains("from-add"));
}

#[test]
fn on_off_emit_shell_snippets() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();

    let on = common::lade(home.path())
        .current_dir(dir.path())
        .arg("on")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let on_text = String::from_utf8_lossy(&on);
    assert!(on_text.contains("set --"));
    assert!(on_text.contains("LADE_SHELL"));

    let off = common::lade(home.path())
        .current_dir(dir.path())
        .arg("off")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let off_text = String::from_utf8_lossy(&off);
    assert!(off_text.contains("LADE_SHELL"));
}

#[test]
fn update_succeeds_with_existing_lock() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["setup", "--harness", "cursor"])
        .assert()
        .success();

    common::lade(home.path())
        .current_dir(dir.path())
        .arg("update")
        .assert()
        .success();
}

#[test]
fn hook_enable_shell_writes_profile() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let bashrc = home.path().join(".bashrc");

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "enable", "--shell"])
        .assert()
        .success();

    let content = fs::read_to_string(&bashrc).unwrap();
    assert!(
        content.contains("lade-do-not-edit"),
        "profile should contain the pre-exec marker: {content}"
    );
    assert!(
        content.contains(" on") || content.contains(" on\""),
        "profile should install the pre-exec snippet: {content}"
    );

    let output = common::lade(home.path())
        .current_dir(dir.path())
        .args(["status", "--json"])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert!(value["hooks"]["preexec"]["installed"].as_bool().unwrap());
}

#[test]
fn help_hides_internal_injection_commands() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let output = common::lade(home.path())
        .current_dir(dir.path())
        .arg("-h")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8_lossy(&output);
    let hidden = ["inject", "hook", "set", "unset"];
    for name in hidden {
        assert!(
            !help
                .lines()
                .any(|line| line.trim_start().starts_with(&format!("{name} "))),
            "{name} should stay hidden: {help}"
        );
    }
    assert!(help.contains("setup"), "{help}");
    assert!(help.contains("remove"), "{help}");
}
