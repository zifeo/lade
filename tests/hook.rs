mod common;

use std::path::Path;
use tempfile::tempdir;

#[test]
fn unread_hook_payload_is_visible_on_stderr() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin("not json at all")
        .assert()
        .success()
        .stdout("")
        .stderr(predicates::str::contains("hook stdin is not JSON"))
        .stderr(predicates::str::contains(
            "The command will run without injection.",
        ));
}

#[test]
fn a_payload_that_is_not_utf8_allows_the_tool_call() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin(b"{\"tool_input\":{\"command\":\"\xff\"}}".to_vec())
        .assert()
        .success()
        .stdout("")
        .stderr(predicates::str::contains("hook stdin is not JSON"));
}

#[test]
fn hook_event_without_command_is_visible_on_stderr() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin(r#"{"tool_name":"Bash","hook_event_name":"PreToolUse"}"#)
        .assert()
        .success()
        .stdout("")
        .stderr(predicates::str::contains("no command to wrap"))
        .stderr(predicates::str::contains(
            "The command will run without injection.",
        ));
}

#[test]
fn an_mcp_verb_the_hook_cannot_plan_is_allowed_with_one_lade_line() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::write_yml(dir.path(), "{}\n");
    std::fs::write(home.path().join("lade-config.json"), "not json").unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin(
            r#"{"tool_name":"mcp__engram__mem_stats","tool_input":{},"hook_event_name":"PreToolUse"}"#,
        )
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "{}", "{stdout}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with("lade: warning: "), "{err}");
}

#[test]
fn a_bare_cd_the_hook_cannot_plan_runs_as_sent() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::write_yml(dir.path(), "\"^cd\":\n  TOKEN: val\n");
    std::fs::write(home.path().join("lade-config.json"), "not json").unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin(r#"{"tool_name":"Bash","tool_input":{"command":"cd sub"},"hook_event_name":"PreToolUse"}"#)
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "", "{stdout}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("lade: warning: "), "{err}");
}

#[test]
fn an_unmatched_line_the_hook_cannot_record_runs_as_sent() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::write_yml_raw(
        dir.path(),
        "\"^terraform\":\n  \".\":\n    log: true\n  TOKEN: val\n",
    );
    std::fs::write(home.path().join("lade-config.json"), "not json").unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin(
            r#"{"tool_name":"Bash","tool_input":{"command":"ls"},"hook_event_name":"PreToolUse"}"#,
        )
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "", "{stdout}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("lade: warning: "), "{err}");
}

#[test]
fn a_line_clap_refuses_is_one_lade_line_for_a_harness() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude", "--no-such-flag"])
        .write_stdin("{}")
        .assert()
        .failure()
        .get_output()
        .clone();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with("lade: error: "), "{err}");
}

#[test]
fn mcp_verb_writes_a_seen_row() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::write_yml(dir.path(), "{}\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin(
            r#"{"tool_name":"mcp__engram__mem_stats","tool_input":{"project":"lade"},"hook_event_name":"PreToolUse","tool_use_id":"toolu_seen"}"#,
        )
        .assert()
        .success();
    let rows = common::log_rows(home.path(), dir.path());
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["kind"], "seen");
    assert_eq!(rows[0]["via"], "mcp");
    assert_eq!(rows[0]["argv"], serde_json::json!({"project": "lade"}));
}

const BROKEN_YAML: &str = "\"touch.*\": [unclosed\n";

/// The hook's stdout and stderr for `payload` in `dir`, whose walk is broken.
fn unloaded_hook(home: &Path, dir: &Path, harness: &str, payload: &str) -> (String, String) {
    let out = common::lade(home)
        .current_dir(dir)
        .args(["hook", "--harness", harness])
        .write_stdin(payload.to_string())
        .assert()
        .success()
        .get_output()
        .clone();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn assert_one_refusal_line(err: &str) {
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with("lade: error: "), "{err}");
    assert!(err.contains("lade.yml"), "{err}");
    assert!(err.contains("Fix the file"), "{err}");
}

#[test]
fn each_harness_denies_a_line_while_lade_yaml_does_not_load() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), BROKEN_YAML).unwrap();
    let bash = r#"{"tool_name":"Bash","tool_input":{"command":"touch ran"},"hook_event_name":"PreToolUse"}"#;
    for harness in ["claude", "codex"] {
        let (out, err) = unloaded_hook(home.path(), dir.path(), harness, bash);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let answer = &parsed["hookSpecificOutput"];
        assert_eq!(answer["hookEventName"], "PreToolUse", "{harness}: {out}");
        assert_eq!(answer["permissionDecision"], "deny", "{harness}: {out}");
        let reason = answer["permissionDecisionReason"].as_str().unwrap();
        assert!(reason.contains("lade.yml"), "{harness}: {reason}");
        assert_one_refusal_line(&err);
    }
    let shell = r#"{"tool_name":"Shell","tool_input":{"command":"touch ran"},"hook_event_name":"preToolUse"}"#;
    let (out, err) = unloaded_hook(home.path(), dir.path(), "cursor", shell);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed["permission"], "deny", "{out}");
    for field in ["user_message", "agent_message"] {
        assert!(
            parsed[field].as_str().unwrap().contains("Fix the file"),
            "{out}"
        );
    }
    assert_one_refusal_line(&err);
    assert!(!dir.path().join("ran").exists());
}

#[test]
fn opencode_gets_a_wrap_that_refuses_the_line_while_lade_yaml_does_not_load() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), BROKEN_YAML).unwrap();
    let payload = r#"{"command":"touch ran","session_id":"s1"}"#;
    let (out, _) = unloaded_hook(home.path(), dir.path(), "opencode", payload);
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let line = parsed["command"].as_str().unwrap();
    assert_eq!(line, "lade --pretool -- 'touch ran' && true");
    let bin = assert_cmd::cargo::cargo_bin("lade");
    let path = format!(
        "{}:{}",
        bin.parent().unwrap().display(),
        std::env::var("PATH").unwrap()
    );
    common::shell_line(home.path(), &["bash"], line)
        .current_dir(dir.path())
        .env("PATH", path)
        .assert()
        .code(1)
        .stderr(predicates::str::contains("lade.yml"));
    assert!(!dir.path().join("ran").exists());
}

#[test]
fn a_lade_verb_runs_through_the_hook_while_lade_yaml_does_not_load() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), BROKEN_YAML).unwrap();
    for command in ["lade status", "lade off", "lade -- touch ran"] {
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": { "command": command },
            "hook_event_name": "PreToolUse",
        });
        let (out, err) = unloaded_hook(home.path(), dir.path(), "claude", &payload.to_string());
        assert_eq!(out, "", "{command}: {out}");
        assert_eq!(err, "", "{command}: {err}");
    }
    let (out, err) = unloaded_hook(home.path(), dir.path(), "claude", "not json at all");
    assert_eq!(out, "", "{out}");
    assert!(err.contains("hook stdin is not JSON"), "{err}");
}

#[test]
fn a_line_moved_into_a_broken_lade_yaml_is_denied() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::write_yml(dir.path(), "{}\n");
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("lade.yml"), BROKEN_YAML).unwrap();
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": "cd sub && touch ran" },
        "hook_event_name": "PreToolUse",
        "cwd": dir.path(),
    });
    let (out, err) = unloaded_hook(home.path(), dir.path(), "claude", &payload.to_string());
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        parsed["hookSpecificOutput"]["permissionDecision"], "deny",
        "{out}"
    );
    assert_one_refusal_line(&err);
}
