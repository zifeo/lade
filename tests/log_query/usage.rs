use super::common;
use super::common::{SECRET, filter_log, inject, log_rows, write_yml};
use std::fs;
use tempfile::tempdir;

#[test]
fn usage_without_window_lists_matched_rules() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo \":\n  API_TOKEN: raw://x\n");
    inject(home.path(), dir.path(), &["echo", "hi"]);
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["usage", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["rule"], "^echo ");
    assert_eq!(rows[0]["count"], 1);
    assert!(
        rows[0]["file"]
            .as_str()
            .unwrap()
            .contains(dir.path().to_str().unwrap()),
        "{}",
        rows[0]["file"]
    );
}

#[test]
fn prune_needs_keep() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "prune"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("--keep"));
}

#[test]
fn prune_keep_deletes_old_rows() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let db = home.path().join("events.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("UPDATE events SET ts = '2020-01-01T00:00:00.000Z'", [])
        .unwrap();
    drop(conn);
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "prune", "--keep", "1d"])
        .assert()
        .success();
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows.as_array().unwrap().len(), 0);
}

#[test]
fn usage_lists_matched_rules_by_frequency() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(
        dir.path(),
        "\"^npm run deploy\":\n  API_TOKEN: raw://x\n\"^other\":\n  OTHER: raw://y\n",
    );
    inject(home.path(), dir.path(), &["bash", "-lc", "npm run deploy"]);
    inject(home.path(), dir.path(), &["npm", "run", "deploy"]);
    inject(home.path(), dir.path(), &["npm", "run", "build"]);
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["usage", "--json", "--since", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["rule"], "^npm run deploy");
    assert_eq!(rows[0]["count"], 1);
    assert_eq!(rows[0]["tags"], serde_json::json!(["env"]));
    inject(home.path(), dir.path(), &["npm", "run", "deploy"]);
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["usage", "--json", "--since", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert_eq!(rows[0]["count"], 2);
}

#[test]
fn log_group_command_orders_by_count() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["ls"]);
    inject(home.path(), dir.path(), &["ls"]);
    inject(home.path(), dir.path(), &["true"]);
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--json", "--group", "command", "--since", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert_eq!(rows[0]["command"], "ls");
    assert_eq!(rows[0]["count"], 2);
    assert_eq!(rows[1]["command"], "true");
    assert_eq!(rows[1]["count"], 1);
}

#[test]
fn usage_empty_window_does_not_dump_catalog() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "\"^npm run deploy\":\n  API_TOKEN: raw://x\n");
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"deploy":"true"}}"#,
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["usage", "--json", "--since", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 0);
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["usage", "--since", "1d"])
        .assert()
        .success()
        .stderr(predicates::str::contains("no events"));
}

#[test]
fn log_filters_kind_and_audience() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(
        dir.path(),
        "\"^echo \":\n  API_TOKEN: tok_example_0000000001\n",
    );
    inject(home.path(), dir.path(), &["true"]);
    inject(home.path(), dir.path(), &["echo", SECRET]);
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["--pretool", "inject", "true"])
        .assert()
        .success();
    let seen = filter_log(home.path(), dir.path(), &["--kind", "seen"]);
    assert!(seen.iter().all(|r| r["kind"] == "seen"), "{seen:?}");
    assert!(seen.iter().any(|r| r["command"] == "true"), "{seen:?}");
    let access = filter_log(home.path(), dir.path(), &["--kind", "access"]);
    assert_eq!(access.len(), 1);
    assert_eq!(access[0]["kind"], "access");
    let denied = filter_log(home.path(), dir.path(), &["--kind", "denied"]);
    assert!(denied.is_empty());
    let human = filter_log(home.path(), dir.path(), &["--audience", "human"]);
    assert!(human.iter().all(|r| r["audience"] == "human"), "{human:?}");
    let agent = filter_log(home.path(), dir.path(), &["--audience", "agent"]);
    assert_eq!(agent.len(), 1);
    assert_eq!(agent[0]["via"], "pretool");
    assert_eq!(agent[0]["audience"], "agent");
}
