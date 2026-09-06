mod common;
use common::{SECRET, inject, log_rows, write_yml, write_yml_raw};
use std::fs;
use std::thread;
use tempfile::tempdir;

#[test]
fn disclaimer_denied_writes_kind() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml_raw(
        dir.path(),
        "\"^echo\":\n  .:\n    log: true\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    );
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "echo hi"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["kind"], "denied");
    assert_eq!(rows[0]["command"], "echo hi");
    assert!(rows[0]["hydrate_ms"].is_null());
    let matches = rows[0]["matches"].as_array().unwrap();
    assert_eq!(matches[0]["rule"], "^echo");
    assert_eq!(matches[0]["bindings"][0]["key"], "SECRET");
    assert_eq!(matches[0]["bindings"][0]["uri"], "val");
    let denied = common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--json", "--since", "1d", "--kind", "denied"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let denied: serde_json::Value = serde_json::from_slice(&denied).unwrap();
    assert_eq!(denied.as_array().unwrap().len(), 1);
}

#[test]
fn write_failure_does_not_change_exit() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    fs::create_dir_all(home.path().join("events.db")).unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "true"])
        .assert()
        .success();
}

#[test]
fn two_writers_do_not_corrupt() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    let home_path = home.path().to_path_buf();
    let dir_path = dir.path().to_path_buf();
    let a = {
        let home = home_path.clone();
        let dir = dir_path.clone();
        thread::spawn(move || {
            common::lade(&home)
                .current_dir(&dir)
                .args(["inject", "true"])
                .assert()
                .success();
        })
    };
    let b = {
        let home = home_path.clone();
        let dir = dir_path.clone();
        thread::spawn(move || {
            common::lade(&home)
                .current_dir(&dir)
                .args(["inject", "true"])
                .assert()
                .success();
        })
    };
    a.join().unwrap();
    b.join().unwrap();
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows.as_array().unwrap().len(), 2);
}

#[test]
fn set_emits_seen_without_event_id() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "echo hi"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(!stdout.contains("LADE_EVENT_ID"), "{stdout}");
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows[0]["kind"], "seen");
    assert_eq!(rows[0]["via"], "preexec");
    assert_eq!(rows[0]["command"], "echo hi");
}

#[test]
fn hook_match_is_silent_inject_writes() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo \":\n  API_TOKEN: raw://x\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .env("CURSOR_VERSION", "1.0")
        .args(["hook"])
        .write_stdin(
            r#"{"tool_name":"Shell","tool_input":{"command":"echo hi"},"hook_event_name":"preToolUse"}"#,
        )
        .assert()
        .success();
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows.as_array().unwrap().len(), 0);
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["--pretool", "inject", "echo", "hi"])
        .assert()
        .success();
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows[0]["kind"], "access");
    assert_eq!(rows[0]["via"], "pretool");
    assert_eq!(rows[0]["audience"], "agent");
    assert_eq!(rows[0]["command"], "echo hi");
}

#[test]
fn hook_no_match_writes_nothing() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml_raw(dir.path(), "{}\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .env("CURSOR_VERSION", "1.0")
        .args(["hook"])
        .write_stdin(
            r#"{"tool_name":"Shell","tool_input":{"command":"echo hi"},"hook_event_name":"preToolUse"}"#,
        )
        .assert()
        .success();
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows.as_array().unwrap().len(), 0);
}

#[test]
fn hook_no_match_walk_log_writes_seen() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml_raw(
        dir.path(),
        "\"^git status\":\n  .:\n    log: true\n  TOKEN: raw://x\n",
    );
    common::lade(home.path())
        .current_dir(dir.path())
        .env("CURSOR_VERSION", "1.0")
        .args(["hook"])
        .write_stdin(
            r#"{"tool_name":"Shell","tool_input":{"command":"echo hi"},"hook_event_name":"preToolUse","conversation_id":"conv_1"}"#,
        )
        .assert()
        .success();
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows[0]["kind"], "seen");
    assert_eq!(rows[0]["via"], "pretool");
    assert_eq!(rows[0]["command"], "echo hi");
    assert_eq!(rows[0]["agent"]["harness"], "cursor");
    assert_eq!(rows[0]["agent"]["session"], "conv_1");
}

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
    let cmd = rows[0]["command"].as_str().unwrap();
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
    let cmd = rows[0]["command"].as_str().unwrap();
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
    let cmds: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["command"].as_str().unwrap())
        .collect();
    assert!(cmds.contains(&"echo hi"), "{cmds:?}");
    assert!(!cmds.iter().any(|c| c.contains("git status")), "{cmds:?}");
    assert!(cmds.contains(&"npm run deploy"), "{cmds:?}");
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
