use super::common;
use super::support::*;
use predicates::prelude::PredicateBooleanExt;
use tempfile::tempdir;

#[test]
fn hook_peels_inject_and_writes_ticket() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo\":\n  SECRET: from_ticket\n");
    let stdout = hook_stdout(
        home.path(),
        dir.path(),
        tmp.path(),
        "lade inject -- echo hi",
    );
    let wrapped = wrap_command(&stdout);
    assert!(!wrapped.contains("lade --pretool inject"), "{wrapped}");
    assert!(wrapped.contains("--pretool="), "{wrapped}");
    assert!(wrapped.contains("'echo hi'"), "{wrapped}");
    let id = ticket_id_from_wrap(&wrapped);
    assert!(ticket_path(home.path(), &id).exists());
}

#[test]
fn wrap_hydrates_from_ticket_without_rematch() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo\":\n  SECRET: from_ticket\n");
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), "echo $SECRET");
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    assert!(ticket_path(home.path(), &id).exists());
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
    assert!(!ticket_path(home.path(), &id).exists());
}

#[test]
fn a_running_command_no_longer_holds_its_ticket() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"^test\":\n  SECRET: from_ticket\n");
    // The wrap uses a ticket only for the line it holds, and the ticket's
    // name is known only once the hook wrote it: the line globs for it.
    let tickets = ticket_path(home.path(), "x").parent().unwrap().to_owned();
    let line = format!("test ! -e {}/*.json && echo gone", tickets.display());
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), &line);
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    assert!(ticket_path(home.path(), &id).exists());
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .args([format!("--pretool={id}"), "inject".into(), line])
        .assert()
        .success()
        .stdout(predicates::str::contains("gone"));
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
    assert!(!ticket_path(home.path(), "Zz9Q").exists());
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
    assert!(ticket_path(home.path(), &id).exists());
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
    assert!(!ticket_path(home.path(), &id).exists());
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
    assert!(!ticket_path(home.path(), &id).exists());
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
    assert!(!ticket_path(home.path(), &id).exists());
    assert!(!output.exists());
    assert!(ticket_ids(home.path()).is_empty());
}

#[test]
fn a_ticket_for_another_line_is_not_used() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo\":\n  SECRET: from_ticket\n");
    let stdout = hook_stdout(home.path(), dir.path(), tmp.path(), "echo hi");
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    write_yml(dir.path(), "\"^nomatch\":\n  SECRET: from_walk\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .args([
            format!("--pretool={id}"),
            "inject".into(),
            "--no-mask".into(),
            "echo other $SECRET".into(),
        ])
        .assert()
        .success()
        .stdout("other\n");
}

#[test]
fn a_ticket_from_another_directory_skips_the_cache() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let source = dir.path().join("source.json");
    std::fs::write(&source, r#"{"token":"from-file"}"#).unwrap();
    write_yml(
        &sub,
        &format!(
            "\"^echo\":\n  KEY: \"file://{}?query=.token\"\n",
            source.display()
        ),
    );
    let line = "echo $KEY";
    let stdout = hook_stdout(home.path(), &sub, tmp.path(), line);
    let id = ticket_id_from_wrap(&wrap_command(&stdout));
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .args([
            format!("--pretool={id}"),
            "inject".into(),
            "--no-mask".into(),
            line.into(),
        ])
        .assert()
        .success()
        .stdout("from-file\n");
    for cwd in [dir.path(), sub.as_path()] {
        common::lade(home.path())
            .current_dir(cwd)
            .args(["ttl"])
            .assert()
            .success()
            .stdout(predicates::str::contains("KEY").not());
    }
    let _ = common::lade(home.path())
        .current_dir(dir.path())
        .args(["ttl", "forget"])
        .output();
}

/// Pre-exec `set`, `lade --` and a pre-tool rewrite give the command the
/// same names and values from one `lade.yaml`.
#[test]
fn the_three_doors_give_the_same_access() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    let source = dir.path().join("source.json");
    std::fs::write(&source, r#"{"token":"from-file"}"#).unwrap();
    write_yml(
        dir.path(),
        &format!(
            "\"^echo\":\n  RAW: raw-value\n  FILE: \"file://{}?query=.token\"\n",
            source.display()
        ),
    );
    let out = dir.path().join("out");
    // `hook_stdout` puts the line in its JSON payload unescaped: no quotes.
    let line = format!("echo $RAW $FILE > {}", out.display());
    let lade = assert_cmd::cargo::cargo_bin("lade");
    let seen = |run: &mut assert_cmd::Command| {
        let _ = std::fs::remove_file(&out);
        run.current_dir(dir.path())
            .env("TMPDIR", tmp.path())
            .assert()
            .success();
        std::fs::read_to_string(&out).unwrap()
    };
    let preexec = seen(&mut common::shell_line(
        home.path(),
        &["bash"],
        &format!("eval \"$('{}' set -- '{line}')\"; {line}", lade.display()),
    ));
    let direct = seen(common::lade(home.path()).args(["--", &line]));
    let wrapped = wrap_command(&hook_stdout(home.path(), dir.path(), tmp.path(), &line));
    assert!(wrapped.contains("--pretool="), "{wrapped}");
    let pretool = seen(&mut common::shell_line(home.path(), &["bash"], &wrapped));
    assert_eq!(preexec, "raw-value from-file\n");
    assert_eq!(direct, preexec);
    assert_eq!(pretool, preexec);
}
