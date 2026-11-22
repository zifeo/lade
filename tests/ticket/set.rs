use super::common;
use super::support::*;
use predicates::prelude::PredicateBooleanExt;
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
}

#[test]
fn a_nested_set_leaves_the_outer_commands_ticket() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    write_yml(dir.path(), "\"mycmd\":\n  SECRET: val\n");
    let set = |lade_t: Option<&str>| {
        let mut cmd = common::lade(home.path());
        cmd.current_dir(dir.path())
            .env("LADE_TICKET_DIR", tickets.path())
            .args(["set", "mycmd"]);
        if let Some(id) = lade_t {
            cmd.env("LADE_T", id);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let outer = common::extract_lade_t(&set(None)).expect("outer LADE_T");
    let outer_file = tickets.path().join(format!("{outer}.json"));
    let before = fs::read(&outer_file).unwrap();

    let stdout = set(Some(&outer));
    let first = stdout.lines().next().unwrap_or_default();
    assert!(first.contains("LADE_T"), "{stdout}");
    let inner = common::extract_lade_t(&stdout).expect("inner LADE_T");
    assert_ne!(inner, outer);
    assert_eq!(fs::read(&outer_file).unwrap(), before);

    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_T", &inner)
        .args(["unset", "mycmd"])
        .assert()
        .success();
    assert!(!tickets.path().join(format!("{inner}.json")).exists());
    assert_eq!(fs::read(&outer_file).unwrap(), before);
}

#[test]
fn a_nested_unset_leaves_the_outer_commands_ticket() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    write_yml(
        dir.path(),
        "\"mycmd\":\n  .:\n    file: out.json\n  SECRET: val\n",
    );
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .args(["set", "mycmd"])
        .output()
        .unwrap()
        .stdout;
    let outer = common::extract_lade_t(&String::from_utf8_lossy(&stdout)).expect("outer LADE_T");
    let outer_file = tickets.path().join(format!("{outer}.json"));
    let out = dir.path().join("out.json");
    assert!(out.exists());
    // The nested shell's `lade set` failed on the yaml, so `LADE_T` still
    // names the outer command's ticket when its `lade unset` runs.
    write_yml(dir.path(), "\"mycmd\": [unclosed\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_T", &outer)
        .args(["unset", "othercmd"])
        .assert()
        .success();
    assert!(outer_file.exists());
    assert!(out.exists());
}

#[test]
fn a_nested_unset_restores_no_snapshot_of_the_outer_command() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    write_yml(dir.path(), "\"mycmd\":\n  EXISTING: after\n  SECRET: val\n");
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("EXISTING", "before")
        .args(["set", "mycmd"])
        .output()
        .unwrap()
        .stdout;
    let stdout = String::from_utf8_lossy(&stdout);
    let outer = common::extract_lade_t(&stdout).expect("outer LADE_T");
    let marker = "export LADE_RESTORE='";
    let rest = &stdout[stdout.find(marker).expect("LADE_RESTORE export") + marker.len()..];
    let restore = &rest[..rest.find('\'').expect("restore payload end")];
    let mise_config = dir.path().join("outer-mise.toml");
    fs::write(&mise_config, "").unwrap();
    // The nested shell's `lade set` failed on the yaml before it printed, so
    // the outer command's `LADE_T`, snapshot and mise config are still there.
    write_yml(dir.path(), "\"mycmd\": [unclosed\n");
    let unset = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_T", &outer)
        .env("LADE_RESTORE", restore)
        .env("LADE_MISE_CONFIG", &mise_config)
        .args(["unset", "othercmd"])
        .output()
        .unwrap();
    assert!(unset.status.success());
    let line = String::from_utf8_lossy(&unset.stdout);
    assert!(!line.contains("EXISTING"), "{line}");
    assert!(!line.contains("SECRET"), "{line}");
    assert!(mise_config.exists());
    assert!(tickets.path().join(format!("{outer}.json")).exists());
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
        serde_json::from_slice(&fs::read(ticket_path(home.path(), &id)).unwrap()).unwrap();
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

#[test]
fn set_skips_a_pending_ticket_for_another_line() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    write_yml(
        dir.path(),
        "\"deploy\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    );
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .args(["set", "deploy"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let id = common::extract_lade_t(&String::from_utf8_lossy(&out.stdout)).expect("LADE_T");
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_T", &id)
        .args(["set", "--", "ls"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(!stdout.contains("export"), "{stdout}");
    assert!(!stderr.contains("Danger!"), "{stderr}");
}

#[test]
fn set_with_a_ticket_from_another_directory_skips_the_cache() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    let source = dir.path().join("source.json");
    fs::write(&source, r#"{"token":"from-file"}"#).unwrap();
    write_yml(
        &sub,
        &format!(
            "\"^echo\":\n  \".\":\n    disclaimer: \"Danger!\"\n  KEY: \"file://{}?query=.token\"\n",
            source.display()
        ),
    );
    let line = "echo $KEY";
    // The withheld line's pending ticket is the one `LADE_T` keeps in the shell.
    let out = common::lade(home.path())
        .current_dir(&sub)
        .env("LADE_TICKET_DIR", tmp.path())
        .args(["set", "--", line])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let id = common::extract_lade_t(&String::from_utf8_lossy(&out.stdout)).expect("LADE_T");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let code: String = stderr[stderr.find("LADE_APPROVE=").expect("approval code") + 13..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tmp.path())
        .env("LADE_T", &id)
        .env("LADE_APPROVE", &code)
        .args(["set", "--", line])
        .assert()
        .success()
        .stdout(predicates::str::contains("export KEY='from-file'"));
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

#[test]
fn a_ticket_holds_sources_not_values() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    let source = dir.path().join("source.json");
    fs::write(&source, format!(r#"{{"token":"{}"}}"#, common::SECRET)).unwrap();
    let uri = format!("file://{}?query=.token", source.display());
    write_yml(dir.path(), &format!("\"mycmd\":\n  SECRET: \"{uri}\"\n"));
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .args(["set", "mycmd"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let id = common::extract_lade_t(&String::from_utf8_lossy(&out)).expect("LADE_T");
    let ticket = fs::read_to_string(tickets.path().join(format!("{id}.json"))).unwrap();
    assert!(ticket.contains(&uri), "{ticket}");
    assert!(!ticket.contains(common::SECRET), "{ticket}");
}
