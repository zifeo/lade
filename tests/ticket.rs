mod common;
use predicates::prelude::PredicateBooleanExt;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn write_yml(dir: &Path, body: &str) {
    fs::write(dir.join("lade.yml"), body).unwrap();
}

fn hook_stdout(home: &Path, dir: &Path, tmp: &Path, command: &str) -> String {
    let payload = format!(
        r#"{{"tool_name":"Shell","tool_input":{{"command":"{command}"}},"hook_event_name":"preToolUse","conversation_id":"conv_1"}}"#
    );
    let out = common::lade(home)
        .current_dir(dir)
        .env("TMPDIR", tmp)
        .env("TMP", tmp)
        .env("TEMP", tmp)
        .env("CURSOR_VERSION", "1.0")
        .args(["hook", "--harness", "cursor"])
        .write_stdin(payload)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8_lossy(&out).into_owned()
}

fn wrap_command(stdout: &str) -> String {
    let parsed: Value = serde_json::from_str(stdout).unwrap();
    parsed["updated_input"]["command"]
        .as_str()
        .or_else(|| parsed["hookSpecificOutput"]["updatedInput"]["command"].as_str())
        .unwrap_or_else(|| panic!("wrap command in hook stdout: {stdout}"))
        .to_string()
}

fn ticket_id_from_wrap(command: &str) -> String {
    let marker = "--pretool=";
    let start = command
        .find(marker)
        .unwrap_or_else(|| panic!("--pretool= in {command}"))
        + marker.len();
    command[start..].chars().take(4).collect()
}

fn ticket_path(tmp: &Path, id: &str) -> PathBuf {
    tmp.join("lade-t").join(format!("{id}.json"))
}

fn ticket_ids(tmp: &Path) -> Vec<String> {
    let dir = tmp.join("lade-t");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let id = name.strip_suffix(".json")?;
            Some(id.to_string())
        })
        .collect();
    ids.sort();
    ids
}

fn log_rows(home: &Path, dir: &Path) -> Value {
    let out = common::lade(home)
        .current_dir(dir)
        .args(["log", "--json", "--since", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn wrap_hydrates_from_ticket_without_rematch() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo\":\n  SECRET: from_ticket\n");
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), "echo hi");
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    assert!(ticket_path(tmp.path(), &id).exists());
    write_yml(dir.path(), "\"^echo\":\n  SECRET: from_walk\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .args([
            format!("--pretool={id}"),
            "inject".into(),
            "--no-mask".into(),
            "echo".into(),
            "$SECRET".into(),
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("from_ticket"))
        .stdout(predicates::str::contains("from_walk").not());
    assert!(!ticket_path(tmp.path(), &id).exists());
}

#[test]
fn missing_ticket_walks_yaml() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo\":\n  SECRET: from_walk\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .args(["--pretool=Zz9Q", "inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("from_walk"));
    assert!(!ticket_path(tmp.path(), "Zz9Q").exists());
}

#[test]
fn child_does_not_see_lade_t() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo\":\n  SECRET: val\n");
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), "echo hi");
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .env("LADE_T", &id)
        .args([
            format!("--pretool={id}"),
            "inject".into(),
            "--no-mask".into(),
            "echo".into(),
            "t=$LADE_T".into(),
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("t=\n"))
        .stdout(predicates::str::contains(&id).not());
}

#[test]
fn direct_inject_ignores_leftover_lade_t() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo\":\n  SECRET: from_ticket\n");
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), "echo hi");
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    write_yml(dir.path(), "\"^echo\":\n  SECRET: from_walk\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .env("LADE_T", &id)
        .args(["inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("from_walk"))
        .stdout(predicates::str::contains("from_ticket").not());
    assert!(ticket_path(tmp.path(), &id).exists());
}

#[test]
fn ticket_unlinked_after_disclaimer_deny() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(
        dir.path(),
        "\"^echo\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    );
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), "echo hi");
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    write_yml(dir.path(), "\"^echo\":\n  SECRET: changed\n");
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .args([
            format!("--pretool={id}"),
            "inject".into(),
            "echo".into(),
            "hi".into(),
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Danger!"), "{stderr}");
    assert!(!ticket_path(tmp.path(), &id).exists());
}

#[test]
fn ticket_unlinked_after_child_fail() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"^false\":\n  SECRET: val\n");
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), "false");
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .args([format!("--pretool={id}"), "inject".into(), "false".into()])
        .assert()
        .failure()
        .code(1);
    assert!(!ticket_path(tmp.path(), &id).exists());
}

#[test]
fn hydrate_failure_unlinks_ticket_and_leaves_no_output_file() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    let missing = dir.path().join("missing.json");
    let output = dir.path().join("out.json");
    let source = missing.to_str().unwrap().replace('\\', "/");
    write_yml(
        dir.path(),
        &format!(
            "\"^echo\":\n  \".\": {{ file: \"out.json\" }}\n  SECRET: \"file://{source}?query=.token\"\n"
        ),
    );
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), "echo hi");
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .args([
            format!("--pretool={id}"),
            "inject".into(),
            "echo".into(),
            "hi".into(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Could not load secrets"), "{stderr}");
    assert!(!ticket_path(tmp.path(), &id).exists());
    assert!(!output.exists());
    assert!(ticket_ids(tmp.path()).is_empty());
}

#[test]
fn set_exports_lade_t() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"mycmd\":\n  SECRET: val\n");
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tmp.path())
        .args(["set", "mycmd"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export SECRET='val'"))
        .stdout(predicates::str::contains("export LADE_T='"))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&stdout);
    let id = common::extract_lade_t(&stdout).expect("LADE_T in set output");
    let pre: Value =
        serde_json::from_slice(&fs::read(tmp.path().join(format!("{id}.json"))).unwrap()).unwrap();
    assert_eq!(pre["via"], "preexec");
    assert_eq!(pre["pending"], serde_json::Value::Null);
    assert!(pre["network_pids"].is_null() || pre["network_pids"].as_array().unwrap().is_empty());
}

#[test]
fn hook_writes_agent_onto_ticket() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(
        dir.path(),
        "\"^echo\":\n  \".\":\n    log: true\n  SECRET: val\n",
    );
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), "echo hi");
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    let pre: Value =
        serde_json::from_slice(&fs::read(ticket_path(tmp.path(), &id)).unwrap()).unwrap();
    assert_eq!(pre["agent"]["harness"], "cursor");
    assert_eq!(pre["agent"]["session"], "conv_1");
    assert!(pre["log"].as_bool().unwrap());
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .args([
            format!("--pretool={id}"),
            "inject".into(),
            "echo".into(),
            "hi".into(),
        ])
        .assert()
        .success();
    let rows = log_rows(home.path(), dir.path());
    assert_eq!(rows[0]["agent"]["harness"], "cursor");
    assert_eq!(rows[0]["agent"]["session"], "conv_1");
}
