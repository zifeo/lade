use super::common;
use super::common::{init_git, inject, log_rows, row_line, write_yml};
use tempfile::tempdir;

#[test]
fn default_window_is_90d_limit_sees_older() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let db = home.path().join("events.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("UPDATE events SET ts = '2020-01-01T00:00:00.000Z'", [])
        .unwrap();
    drop(conn);
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 0);
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--json", "--limit", "10"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let all: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(all.as_array().unwrap().len(), 1);
}

#[test]
fn until_excludes_recent_rows() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--json", "--until", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 0);
}

#[test]
fn log_and_usage_scope_to_git_root() {
    let home = tempdir().unwrap();
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    write_yml(a.path(), "{}\n");
    write_yml(b.path(), "{}\n");
    init_git(a.path());
    init_git(b.path());
    inject(home.path(), a.path(), &["ls"]);
    inject(home.path(), b.path(), &["true"]);
    let a_rows = log_rows(home.path(), a.path());
    let cmds: Vec<&str> = a_rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["command"].as_str().unwrap())
        .collect();
    assert!(cmds.contains(&"ls"), "{cmds:?}");
    assert!(!cmds.contains(&"true"), "{cmds:?}");
}

#[test]
fn usage_and_log_all_reads_every_repo() {
    let home = tempdir().unwrap();
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    write_yml(a.path(), "\"^echo a\":\n  A: raw://a\n");
    write_yml(b.path(), "\"^echo b\":\n  B: raw://b\n");
    init_git(a.path());
    init_git(b.path());
    inject(home.path(), a.path(), &["echo", "a"]);
    inject(home.path(), b.path(), &["echo", "b"]);
    let usage = common::lade(home.path())
        .current_dir(a.path())
        .args(["usage", "--json", "--all", "--since", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rules: Vec<serde_json::Value> = serde_json::from_slice(&usage).unwrap();
    let names: Vec<&str> = rules.iter().map(|r| r["rule"].as_str().unwrap()).collect();
    assert!(names.contains(&"^echo a"), "{names:?}");
    assert!(names.contains(&"^echo b"), "{names:?}");
    let log = common::lade(home.path())
        .current_dir(a.path())
        .args(["log", "--json", "--all", "--since", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&log).unwrap();
    let cmds: Vec<String> = rows.iter().map(row_line).collect();
    assert!(cmds.iter().any(|c| c == "echo a"), "{cmds:?}");
    assert!(cmds.iter().any(|c| c == "echo b"), "{cmds:?}");
}

#[test]
fn usage_and_log_path_scopes_to_that_root() {
    let home = tempdir().unwrap();
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    write_yml(a.path(), "\"^echo a\":\n  A: raw://a\n");
    write_yml(b.path(), "\"^echo b\":\n  B: raw://b\n");
    init_git(a.path());
    init_git(b.path());
    inject(home.path(), a.path(), &["echo", "a"]);
    inject(home.path(), b.path(), &["echo", "b"]);
    let usage = common::lade(home.path())
        .current_dir(a.path())
        .args([
            "usage",
            "--json",
            "--path",
            b.path().to_str().unwrap(),
            "--since",
            "1d",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rules: Vec<serde_json::Value> = serde_json::from_slice(&usage).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["rule"], "^echo b");
    let log = common::lade(home.path())
        .current_dir(a.path())
        .args([
            "log",
            "--json",
            "--path",
            b.path().to_str().unwrap(),
            "--since",
            "1d",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&log).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(row_line(&rows[0]), "echo b");
}
