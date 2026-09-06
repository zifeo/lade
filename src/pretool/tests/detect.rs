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
fn test_detect_pi_env() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", None),
            ("CODEX_THREAD_ID", None),
            ("CODEX_HOME", None),
            ("PI_HOME", Some("/tmp/pi")),
            ("OPENCODE", None),
        ],
        || {
            assert_eq!(detect_platform(&json!({})), Some(Platform::Pi));
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
