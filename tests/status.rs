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
        .stdout(predicates::str::starts_with("Lade "))
        .stdout(predicates::str::is_match(r"age plugin +").unwrap())
        .stdout(predicates::str::is_match(r"latest +\(unknown, the hub is not running\)").unwrap())
        .stdout(predicates::str::is_match(r"config +ok \(\d+ rules\)").unwrap())
        .stdout(predicates::str::is_match(r"inject wrap +skips startup files").unwrap())
        .stdout(predicates::str::is_match(r"log +").unwrap())
        .stdout(predicates::str::contains("0 events, 0 B"))
        .stdout(predicates::str::is_match(r"\nHub\n +state +down").unwrap());
}

#[test]
fn test_status_fails_on_an_invalid_max_cache() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    fs::write(
        home.path().join("lade-config.json"),
        r#"{"user":null,"max_cache":"forever"}"#,
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .failure()
        .stdout(predicates::str::is_match(r"max cache +invalid, cache off: ").unwrap());
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
    assert!(value.get("age_plugin").is_some());
    assert!(value["age_plugin"].get("present").is_some());
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
    assert!(value.get("skills").is_none());
    assert!(value.get("log").is_some());
    assert!(value["log"].get("path").is_some());
    assert_eq!(value["log"]["events"], 0);
    assert_eq!(value["log"]["bytes"], 0);
    assert!(value.get("hub").is_some());
    assert_eq!(value["hub"]["state"], "down");
    assert!(value["hub"]["pid"].is_null());
    assert_eq!(value["hub"]["secrets"], 0);
    assert_eq!(value["hub"]["tickets"], 0);
    assert!(!home.path().join("events.db").is_file());
    assert!(value["project_config"]["error"].is_null());
    assert!(value["project_config"].get("vault_clis").is_some());
    assert!(value["project_config"].get("providers").is_some());
    assert!(
        value["project_config"]["providers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // The hub holds the version ledger, and no hub runs here.
    assert!(value["version"]["latest"].is_null());
    assert_eq!(value["version"]["update_available"], false);
    assert!(value["version"]["last_check"].is_null());
    assert!(value["version"]["check_error"].is_null());
}

#[test]
fn test_status_json_reports_provider_batch_unit() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: awssm://us-east-1/app/db\n",
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
    let providers = value["project_config"]["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0]["scheme"], "awssm");
    assert_eq!(providers[0]["transport"], "cli");
    assert_eq!(providers[0]["batch_unit"], "(region, name)");
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
    common::init_git(dir.path());
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    fs::write(
        dir.path().join(".claude").join("settings.local.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#,
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .stdout(predicates::str::contains("\nPre-exec ("))
        .stdout(
            predicates::str::is_match(r"\nPre-tool\n +HARNESS +SCOPE +STATE +PATH\n").unwrap(),
        )
        .stdout(
            predicates::str::is_match(
                r"\n +Claude Code +machine +missing +\S+\n +Claude Code +repo +not current +\S*\.claude/settings\.local\.json\n",
            )
            .unwrap(),
        )
        .stdout(predicates::str::contains("! Claude Code (this repo, "))
        .stdout(predicates::str::contains(
            "the Lade hook differs from what this Lade writes. `lade hook enable --harness claude` rewrites it.",
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
            .ends_with(".claude/settings.local.json")
    );
}

#[test]
fn test_status_flags_a_claude_hook_left_in_settings_json() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::init_git(dir.path());
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    fs::write(
        dir.path().join(".claude").join("settings.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook --harness claude"}]}]}}"#,
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .stdout(predicates::str::contains(
            "`lade hook enable --harness claude` moves it to settings.local.json",
        ));
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
    common::init_git(dir.path());
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
    common::lade_in(home.path(), "fish")
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .stdout(
            predicates::str::is_match(r"inject wrap +skips startup files \(config\.fish present\)")
                .unwrap(),
        );
}
