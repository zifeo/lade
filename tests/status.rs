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
        .stdout(predicates::str::contains("project config: ok"));
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
    assert!(value.get("project_config").is_some());
    assert!(value.get("ok").is_some());
    assert!(value["project_config"]["error"].is_null());
}
