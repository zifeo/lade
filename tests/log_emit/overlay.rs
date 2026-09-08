use super::common;
use super::common::{SECRET, inject, log_rows, row_line, write_yml, write_yml_raw};
use std::fs;
use tempfile::tempdir;

#[test]
fn log_json_includes_agent_from_env() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo \":\n  API_TOKEN: raw://x\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .env("CODEX_THREAD_ID", "thr_json")
        .args(["inject", "echo", "hi"])
        .assert()
        .success();
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows[0]["agent"]["harness"], "codex");
    assert_eq!(rows[0]["agent"]["session"], "thr_json");
}

#[test]
fn absent_log_writes_nothing() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml_raw(dir.path(), "\"^echo \":\n  API_TOKEN: raw://x\n");
    inject(home.path(), dir.path(), &["echo", "hi"]);
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows.as_array().unwrap().len(), 0);
}

#[test]
fn log_false_writes_nothing() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml_raw(
        dir.path(),
        ".:\n  .:\n    log: true\n\"^git status\":\n  .:\n    log: false\n",
    );
    inject(home.path(), dir.path(), &["git", "status"]);
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows.as_array().unwrap().len(), 0);
}

#[test]
fn dot_log_then_later_secret_is_access() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml_raw(
        dir.path(),
        ".:\n  .:\n    log: true\n  DEFAULT: raw://default\n\"^echo \":\n  API_TOKEN: tok_example_0000000001\n",
    );
    inject(home.path(), dir.path(), &["echo", SECRET]);
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows[0]["kind"], "access");
    let cmd = row_line(&rows[0]);
    assert!(cmd.contains("${API_TOKEN}"), "{cmd}");
    assert!(!cmd.contains(SECRET), "{cmd}");
}

#[test]
fn child_secrets_inherit_parent_dot_log() {
    let home = tempdir().unwrap();
    let parent = tempdir().unwrap();
    write_yml_raw(parent.path(), ".:\n  .:\n    log: true\n");
    let child = parent.path().join("app");
    fs::create_dir(&child).unwrap();
    write_yml_raw(&child, "\"^echo \":\n  API_TOKEN: tok_example_0000000001\n");
    inject(home.path(), &child, &["echo", SECRET]);
    let rows = log_rows(home.path(), &child);
    assert_eq!(rows[0]["kind"], "access");
    let cmd = row_line(&rows[0]);
    assert!(cmd.contains("${API_TOKEN}"), "{cmd}");
}

#[test]
fn overlay_log_parent_then_child() {
    let home = tempdir().unwrap();
    let parent = tempdir().unwrap();
    write_yml_raw(parent.path(), ".:\n  .:\n    log: true\n");
    let child = parent.path().join("app");
    fs::create_dir(&child).unwrap();
    write_yml_raw(
        &child,
        "\"^git status\":\n  .:\n    log: false\n\"^npm run deploy\":\n  API_TOKEN: raw://x\n",
    );
    inject(home.path(), &child, &["echo", "hi"]);
    inject(home.path(), &child, &["git", "status"]);
    inject(home.path(), &child, &["npm", "run", "deploy"]);
    let rows = log_rows(home.path(), &child);
    let cmds: Vec<String> = rows.as_array().unwrap().iter().map(row_line).collect();
    assert!(cmds.iter().any(|c| c == "echo hi"), "{cmds:?}");
    assert!(!cmds.iter().any(|c| c.contains("git status")), "{cmds:?}");
    assert!(cmds.iter().any(|c| c == "npm run deploy"), "{cmds:?}");
}

#[test]
fn status_json_counts_events_after_write() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["status", "--json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["log"]["events"], 1);
    assert!(value["log"]["bytes"].as_u64().unwrap() > 0);
    assert!(
        value["log"]["path"]
            .as_str()
            .unwrap()
            .ends_with("events.db"),
        "{}",
        value["log"]["path"]
    );
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .stdout(predicates::str::contains("1 events"));
}

#[test]
fn dot_log_records_ls_and_inject() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["ls"]);
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows[0]["command"], "ls");
    assert_eq!(rows[0]["kind"], "seen");
}
