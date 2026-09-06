use super::super::handle;
use super::super::platform::{Platform, collect_agent};
use super::{
    AGENT_ENV, assert_wraps_with_ticket, test_config, with_cursor_env, with_ticket_tmpdir,
};
use crate::config::{Audience, LadeFile};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn test_agent_when_wraps() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let dir = tempdir().unwrap();
            std::fs::write(
                dir.path().join("lade.yml"),
                "\"^echo\":\n  \".\":\n    when: agent\n  KEY: val\n",
            )
            .unwrap();
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let input = r#"{"tool_input": {"command": "echo hello"}}"#;
            let result = handle(&config, input, Audience::Agent, None).unwrap();
            assert_wraps_with_ticket(&result, "echo hello");
        });
    });
}

#[test]
fn test_human_when_does_not_wrap() {
    with_cursor_env(|| {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"^echo\":\n  \".\":\n    when: human\n  KEY: val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn harness_flag_selects_cursor_without_env() {
    temp_env::with_vars(AGENT_ENV, || {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_input":{"command":"echo hello"},"hook_event_name":"preToolUse"}"#;
            let result = handle(&config, input, Audience::Agent, Some("cursor")).unwrap();
            assert!(result.contains("updated_input"));
            assert!(!result.contains("updatedInput"));
        });
    });
}

#[test]
fn harness_opencode_uses_native_command_payload() {
    temp_env::with_vars(AGENT_ENV, || {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"command":"echo hello","session_id":"ses_1"}"#;
            let result = handle(&config, input, Audience::Agent, Some("opencode")).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
            let command = parsed["command"].as_str().expect("native command");
            assert_wraps_with_ticket(command, "echo hello");
            assert!(!result.contains("updatedInput"));
            assert!(!result.contains("updated_input"));
            assert!(!result.contains("hookSpecificOutput"));
        });
    });
}

#[test]
fn unknown_harness_and_bad_json_do_not_fail() {
    temp_env::with_vars(AGENT_ENV, || {
        let (config, _dir) = test_config("^echo");
        assert_eq!(
            handle(&config, "not-json", Audience::Agent, Some("nope")).unwrap(),
            ""
        );
    });
}

#[test]
fn collect_agent_reads_cursor_model_and_session() {
    let agent = collect_agent(
        Some(Platform::Cursor),
        &json!({
            "model_id": "grok-4.5",
            "model": "auto",
            "conversation_id": "conv_9"
        }),
    );
    assert_eq!(agent["harness"], "cursor");
    assert_eq!(agent["model"], "grok-4.5");
    assert_eq!(agent["session"], "conv_9");
}

#[test]
fn collect_agent_drops_auto_model() {
    let agent = collect_agent(
        Some(Platform::Codex),
        &json!({"model": "auto", "session_id": "s1"}),
    );
    assert_eq!(agent["harness"], "codex");
    assert!(agent.get("model").is_none());
    assert_eq!(agent["session"], "s1");
}

#[test]
fn collect_agent_reads_opencode_session_id() {
    let agent = collect_agent(Some(Platform::OpenCode), &json!({"sessionID": "ses_1"}));
    assert_eq!(agent["harness"], "opencode");
    assert_eq!(agent["session"], "ses_1");
}

#[test]
fn ticket_write_failure_allows() {
    with_cursor_env(|| {
        let tmp = tempdir().unwrap();
        let blocker = tmp.path().join("not-a-dir");
        std::fs::write(&blocker, "x").unwrap();
        let blocker = blocker.to_str().unwrap();
        temp_env::with_var("LADE_TICKET_DIR", Some(blocker), || {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_input":{"command":"echo hello"},"hook_event_name":"preToolUse"}"#;
            let result = handle(&config, input, Audience::Agent, Some("cursor")).unwrap();
            assert!(result.contains("allow"));
            assert!(!result.contains("--pretool"));
        });
    });
}
