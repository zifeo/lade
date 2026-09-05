mod common;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_bench_json_splits_incompressible_and_variable() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^echo raw\":\n  A: one\n\"^echo sh\":\n  B: sh://printf x\n",
    )
    .unwrap();
    let output = common::lade(home.path())
        .current_dir(dir.path())
        .args(["bench", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("bench --json must emit valid JSON");
    assert!(value.get("incompressible").is_some());
    assert!(value["incompressible"]["parse_ms"].as_f64().is_some());
    assert!(value["incompressible"]["match_ms"].as_f64().is_some());
    assert_eq!(value["incompressible"]["files"], 1);
    assert_eq!(value["incompressible"]["rules"], 2);
    assert!(value["total_ms"].as_f64().is_some());
    let rules = value["rules"].as_array().expect("rules array");
    assert_eq!(rules.len(), 2);
    let raw = rules
        .iter()
        .find(|rule| rule["pattern"] == "^echo raw")
        .expect("raw rule");
    assert_eq!(raw["when"], "always");
    assert_eq!(raw["providers"][0], "raw");
    assert!(raw["error"].is_null());
    assert!(raw["hydrate_ms"].as_f64().is_some());
    let sh = rules
        .iter()
        .find(|rule| rule["pattern"] == "^echo sh")
        .expect("sh rule");
    assert_eq!(sh["providers"][0], "sh");
    assert!(sh["error"].is_null());
    let stdout = String::from_utf8_lossy(&output);
    assert!(
        !stdout.contains("one"),
        "bench must not print secret values: {stdout}"
    );
}

#[test]
fn test_bench_human_has_both_cost_sections() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(dir.path().join("lade.yml"), "\"^echo raw\":\n  A: one\n").unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("bench")
        .assert()
        .success()
        .stdout(predicates::str::contains("incompressible"))
        .stdout(predicates::str::contains("parse:"))
        .stdout(predicates::str::contains("match:"))
        .stdout(predicates::str::contains("variable"))
        .stdout(predicates::str::contains("^echo raw"))
        .stdout(predicates::str::contains("raw"))
        .stdout(predicates::str::contains("total:"));
}

#[test]
fn test_bench_reports_hydrate_error_per_rule() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^echo bad\":\n  A: op://bad\n",
    )
    .unwrap();
    let output = common::lade(home.path())
        .current_dir(dir.path())
        .args(["bench", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("bench --json must emit valid JSON");
    let err = value["rules"][0]["error"].as_str().expect("hydrate error");
    assert!(err.contains("cannot parse") || err.contains("1Password"));
    assert!(!err.contains('\n'));
    assert!(value["rules"][0]["hydrate_ms"].as_f64().is_some());
}

#[test]
fn test_bench_human_puts_error_on_next_line() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^echo bad\":\n  A: op://bad\n",
    )
    .unwrap();
    let stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .arg("bench")
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .into_owned();
    let error_line = stdout
        .lines()
        .find(|line| line.starts_with("    error  "))
        .expect("indented error line");
    assert!(!error_line.contains("^echo bad"));
}

#[test]
fn test_bench_hydrate_timeout_flag() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^echo slow\":\n  A: sh://sleep 8\n",
    )
    .unwrap();
    let started = std::time::Instant::now();
    let output = common::lade(home.path())
        .current_dir(dir.path())
        .args(["bench", "--json", "--timeout", "1s"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "1s cap should finish before 3s, took {elapsed:?}"
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("bench --json must emit valid JSON");
    assert_eq!(value["timeout_ms"].as_f64(), Some(1000.0));
    assert_eq!(value["rules"][0]["error"], "timeout 1s");
    assert!(value["rules"][0]["hydrate_ms"].as_f64().unwrap() >= 1000.0);
}
