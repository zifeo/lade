use super::common;
use super::support::*;
use predicates::prelude::PredicateBooleanExt;
use tempfile::tempdir;

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
