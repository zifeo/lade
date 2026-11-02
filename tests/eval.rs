mod common;

use predicates::prelude::*;

#[test]
fn eval_prints_file_uri_and_writes_diary() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("secret.json");
    std::fs::write(&key, r#"{"token":"eval-secret-value"}"#).unwrap();
    let uri = format!("file://{}?query=.token", key.display());

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["eval", &uri])
        .assert()
        .success()
        .stdout(predicate::str::contains("eval-secret-value"));

    let rows = common::log_rows(home.path(), dir.path());
    let items = rows.as_array().expect("log array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "access");
    assert_eq!(items[0]["command"], "eval");
    let dump = serde_json::to_string(&items[0]).unwrap();
    assert!(dump.contains(&uri), "{dump}");
    assert!(
        !dump.contains("eval-secret-value"),
        "diary stored the secret: {dump}"
    );
}

#[test]
fn eval_access_command_writes_diary_name() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("secret.json");
    std::fs::write(&key, r#"{"token":"eval-secret-value"}"#).unwrap();
    let uri = format!("file://{}?query=.token", key.display());

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["eval", "--access-command", "age-plugin-lade", "--", &uri])
        .assert()
        .success()
        .stdout(predicate::str::contains("eval-secret-value"));

    let rows = common::log_rows(home.path(), dir.path());
    let items = rows.as_array().expect("log array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "access");
    assert_eq!(items[0]["command"], "age-plugin-lade");
}

#[test]
fn eval_unknown_uri_is_literal() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["eval", "not-a-scheme"])
        .assert()
        .success()
        .stdout(predicate::eq("not-a-scheme\n"));
}

fn eval_value(home: &std::path::Path, dir: &std::path::Path, args: &[&str]) -> String {
    let out = common::lade(home)
        .current_dir(dir)
        .arg("eval")
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim_end().to_string()
}

fn stop_hub(home: &std::path::Path, dir: &std::path::Path) {
    common::lade(home)
        .current_dir(dir)
        .args(["ttl", "forget"])
        .assert()
        .success();
}

/// A value file and its `file://` URI. The value changes between evals.
fn secret_file(dir: &std::path::Path) -> (std::path::PathBuf, String) {
    let key = dir.join("secret.json");
    std::fs::write(&key, r#"{"token":"first-value"}"#).unwrap();
    let uri = format!("file://{}?query=.token", key.display());
    (key, uri)
}

#[test]
fn eval_ttl_keeps_the_value_in_the_hub() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (key, uri) = secret_file(dir.path());
    assert_eq!(
        eval_value(home.path(), dir.path(), &["--ttl", "5m", &uri]),
        "first-value"
    );
    std::fs::write(&key, r#"{"token":"second-value"}"#).unwrap();
    let second = eval_value(home.path(), dir.path(), &["--ttl", "5m", &uri]);
    let listed = common::lade(home.path())
        .current_dir(dir.path())
        .args(["ttl", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let without_flag = eval_value(home.path(), dir.path(), &[&uri]);
    stop_hub(home.path(), dir.path());
    assert_eq!(second, "first-value");
    assert_eq!(without_flag, "second-value");
    let listed: serde_json::Value = serde_json::from_slice(&listed).unwrap();
    assert_eq!(listed["keys"][0]["name"], uri.as_str(), "{listed}");
    let rows = common::log_rows(home.path(), dir.path());
    let cached: Vec<bool> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["matches"][0]["bindings"][0]["cached"] == true)
        .collect();
    assert_eq!(cached.iter().filter(|hit| **hit).count(), 1, "{rows}");
}

#[test]
fn eval_without_ttl_never_starts_the_hub() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_, uri) = secret_file(dir.path());
    eval_value(home.path(), dir.path(), &[&uri]);
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["ttl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hub: down"));
}

#[test]
fn eval_ttl_respects_max_cache_off() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("lade-config.json"),
        r#"{"user":null,"max_cache":"0"}"#,
    )
    .unwrap();
    let (key, uri) = secret_file(dir.path());
    eval_value(home.path(), dir.path(), &["--ttl", "5m", &uri]);
    std::fs::write(&key, r#"{"token":"second-value"}"#).unwrap();
    let second = eval_value(home.path(), dir.path(), &["--ttl", "5m", &uri]);
    stop_hub(home.path(), dir.path());
    assert_eq!(second, "second-value");
}

#[test]
fn eval_ttl_off_fetches_every_time() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (key, uri) = secret_file(dir.path());
    eval_value(home.path(), dir.path(), &["--ttl", "off", &uri]);
    std::fs::write(&key, r#"{"token":"second-value"}"#).unwrap();
    assert_eq!(
        eval_value(home.path(), dir.path(), &["--ttl", "off", &uri]),
        "second-value"
    );
}

#[test]
fn eval_ttl_rejects_a_bad_duration_and_raw_uris() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["eval", "--ttl", "25h", "op://vault/item/field"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("24h"));
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["eval", "--ttl", "5m", "raw://literal"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("never cached"));
}
