use super::super::handle;
use super::super::platform::{Platform, detect_platform};
use super::{AGENT_ENV, test_config, with_cursor_env};
use crate::config::Audience;
use serde_json::json;

#[test]
fn test_detect_cursor() {
    with_cursor_env(|| {
        assert_eq!(detect_platform(&json!({})), Some(Platform::Cursor));
    });
}

#[test]
fn test_detect_claude() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", Some("/tmp")),
            ("CODEX_THREAD_ID", None),
            ("CODEX_SANDBOX", None),
            ("CODEX_HOME", None),
            ("PI_HOME", None),
            ("PI_CODING_AGENT", None),
            ("OPENCODE", None),
            ("OPENCODE_DIR", None),
        ],
        || {
            assert_eq!(detect_platform(&json!({})), Some(Platform::ClaudeCode));
        },
    );
}

#[test]
fn test_detect_codex_env() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", None),
            ("CODEX_THREAD_ID", Some("thr_1")),
            ("CODEX_SANDBOX", None),
            ("CODEX_HOME", None),
            ("PI_HOME", None),
            ("OPENCODE", None),
        ],
        || {
            assert_eq!(detect_platform(&json!({})), Some(Platform::Codex));
        },
    );
}

#[test]
fn test_detect_opencode_payload() {
    temp_env::with_vars(AGENT_ENV, || {
        let input = json!({
            "hook_event_name": "PreToolUse",
            "hook_source": "opencode-plugin",
            "tool_input": {"command": "echo hello"}
        });
        assert_eq!(detect_platform(&input), Some(Platform::OpenCode));
    });
}

#[test]
fn test_detect_unknown_is_none() {
    temp_env::with_vars(AGENT_ENV, || {
        assert_eq!(detect_platform(&json!({})), None);
    });
}

#[test]
fn mcp_verb_without_command_allows_and_does_not_wrap() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("^engram");
        let input = r#"{"tool_name":"MCP:mem_stats","mcp_server_name":"engram","hook_event_name":"preToolUse"}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
        assert!(!result.contains("--pretool"));
    });
}

#[test]
fn before_mcp_does_not_rewrite_launch_command() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("^engram");
        let input = r#"{"hook_event_name":"beforeMCPExecution","tool_name":"MCP:mem_stats","mcp_server_name":"engram","command":"engram"}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
        assert!(!result.contains("--pretool"));
    });
}

#[test]
fn post_mcp_event_allows_without_wrap() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("^engram");
        let input = r#"{"hook_event_name":"afterMCPExecution","tool_name":"MCP:mem_stats"}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("--pretool"));
    });
}

#[test]
fn test_no_command_allows() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("echo");
        let result = handle(&config, "{}", Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
    });
}

#[test]
fn test_no_match_allows() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("^terraform");
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
    });
}
