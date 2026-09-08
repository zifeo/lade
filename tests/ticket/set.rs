use super::common;
use super::support::*;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

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
