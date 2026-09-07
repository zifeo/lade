mod common;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_status_reports_version_and_project() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .stdout(predicates::str::contains("lade version:"))
        .stdout(predicates::str::contains("latest:"))
        .stdout(predicates::str::contains("tried"))
        .stdout(predicates::str::contains("project config: ok"))
        .stdout(predicates::str::contains(
            "inject wrap: skips startup files",
        ))
        .stdout(predicates::str::contains("log:"))
        .stdout(predicates::str::contains("0 events, 0 B"));
}

#[test]
fn test_status_json_is_valid_with_expected_keys() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    let output = common::lade(home.path())
        .current_dir(dir.path())
        .args(["status", "--json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("status --json must emit valid JSON");
    assert!(value.get("version").is_some());
    assert!(value.get("global_config").is_some());
    assert!(value.get("hooks").is_some());
    assert!(value["hooks"].get("preexec").is_some());
    assert!(value["hooks"].get("pretool").is_some());
    assert!(value["hooks"]["pretool"].get("cursor").is_some());
    assert!(value["hooks"]["pretool"].get("claude").is_some());
    assert!(value["hooks"]["pretool"].get("codex").is_some());
    assert!(value["hooks"]["pretool"].get("opencode").is_some());
    assert!(value["hooks"]["pretool"].get("pi").is_none());
    assert!(value.get("project_config").is_some());
    assert!(value.get("ok").is_some());
    assert_eq!(
        value["hooks"]["preexec"]["inject_skips_startup_files"],
        true
    );
    assert!(value.get("skills").is_some());
    assert!(value["skills"].get("cursor").is_some());
    assert!(value.get("log").is_some());
    assert!(value["log"].get("path").is_some());
    assert_eq!(value["log"]["events"], 0);
    assert_eq!(value["log"]["bytes"], 0);
    assert!(!home.path().join("events.db").is_file());
    assert!(value["project_config"]["error"].is_null());
    assert!(value["version"]["latest"].is_null());
    assert_eq!(value["version"]["update_available"], false);
    assert!(
        value["version"]["last_check"]
            .as_str()
            .unwrap()
            .starts_with("2099-01-01")
    );
}

#[test]
fn test_status_reports_project_pretool_hook() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    fs::write(
        dir.path().join(".claude").join("settings.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#,
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .stdout(predicates::str::contains("preexec shell hooks"))
        .stdout(predicates::str::contains("preTool hooks"))
        .stdout(predicates::str::contains(
            "drift: run `lade install` to refresh stale hooks and skills",
        ));
    let output = common::lade(home.path())
        .current_dir(dir.path())
        .args(["status", "--json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("status --json must emit valid JSON");
    assert_eq!(
        value["hooks"]["pretool"]["claude"]["project"]["installed"],
        true
    );
    assert_eq!(
        value["hooks"]["pretool"]["claude"]["project"]["current"],
        false
    );
    assert!(
        value["hooks"]["pretool"]["claude"]["project"]["path"]
            .as_str()
            .unwrap()
            .ends_with(".claude/settings.json")
    );
}

#[test]
fn test_status_reports_project_codex_pretool_hook() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join(".codex")).unwrap();
    fs::write(
        dir.path().join(".codex").join("hooks.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#,
    )
    .unwrap();
    let output = common::lade(home.path())
        .current_dir(dir.path())
        .args(["status", "--json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("status --json must emit valid JSON");
    assert_eq!(
        value["hooks"]["pretool"]["codex"]["project"]["installed"],
        true
    );
    assert_eq!(
        value["hooks"]["pretool"]["codex"]["project"]["current"],
        false
    );
    assert!(
        value["hooks"]["pretool"]["codex"]["project"]["path"]
            .as_str()
            .unwrap()
            .ends_with(".codex/hooks.json")
    );
}

#[test]
fn test_status_names_present_fish_startup_file() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::create_dir_all(home.path().join(".config/fish")).unwrap();
    fs::write(home.path().join(".config/fish/config.fish"), "set -x X 1\n").unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_SHELL", "fish")
        .arg("status")
        .assert()
        .stdout(predicates::str::contains(
            "inject wrap: skips startup files (config.fish present)",
        ));
}
