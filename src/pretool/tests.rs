use super::handle;
use super::platform::{Platform, detect_platform};
use crate::config::{Audience, Config, LadeFile};
use serde_json::json;
use tempfile::{TempDir, tempdir};

const AGENT_ENV: [(&str, Option<&str>); 9] = [
    ("CURSOR_VERSION", None),
    ("CLAUDE_PROJECT_DIR", None),
    ("CODEX_THREAD_ID", None),
    ("CODEX_SANDBOX", None),
    ("CODEX_HOME", None),
    ("PI_HOME", None),
    ("PI_CODING_AGENT", None),
    ("OPENCODE", None),
    ("OPENCODE_DIR", None),
];

fn test_config(pattern: &str) -> (Config, TempDir) {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        format!("\"{}\":\n  KEY: val\n", pattern),
    )
    .unwrap();
    (LadeFile::build(dir.path().to_path_buf()).unwrap(), dir)
}

fn test_config_with_disclaimer(pattern: &str, disclaimer: &str) -> (Config, TempDir) {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"{}\":\n  \".\":\n    disclaimer: \"{}\"\n  KEY: val\n",
            pattern, disclaimer
        ),
    )
    .unwrap();
    (LadeFile::build(dir.path().to_path_buf()).unwrap(), dir)
}

fn with_cursor_env<F: FnOnce()>(f: F) {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", Some("1.0")),
            ("CLAUDE_PROJECT_DIR", None),
            ("CODEX_THREAD_ID", None),
            ("CODEX_SANDBOX", None),
            ("CODEX_HOME", None),
            ("PI_HOME", None),
            ("PI_CODING_AGENT", None),
            ("OPENCODE", None),
            ("OPENCODE_DIR", None),
        ],
        f,
    );
}

#[test]
fn test_detect_cursor() {
    with_cursor_env(|| {
        assert_eq!(detect_platform(&json!({})).unwrap(), Platform::Cursor);
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
            assert_eq!(detect_platform(&json!({})).unwrap(), Platform::ClaudeCode);
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
            assert_eq!(detect_platform(&json!({})).unwrap(), Platform::Codex);
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
            assert_eq!(detect_platform(&json!({})).unwrap(), Platform::Pi);
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
        assert_eq!(detect_platform(&input).unwrap(), Platform::OpenCode);
    });
}

#[test]
fn test_detect_unknown_fails() {
    temp_env::with_vars(AGENT_ENV, || {
        assert!(detect_platform(&json!({})).is_err());
    });
}

#[test]
fn test_no_command_allows() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("echo");
        let result = handle(&config, "{}", Audience::Agent).unwrap();
        assert!(result.contains("allow"));
    });
}

#[test]
fn test_no_match_allows() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("^terraform");
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
    });
}

#[test]
fn test_match_wraps_cursor() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("^echo");
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("--pretool 'echo hello'"));
        assert!(result.contains("updated_input"));
        assert!(!result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_match_wraps_claude() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", Some("/tmp")),
            ("CODEX_THREAD_ID", None),
            ("PI_HOME", None),
            ("OPENCODE", None),
        ],
        || {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_input": {"command": "echo hello"}}"#;
            let result = handle(&config, input, Audience::Agent).unwrap();
            assert!(result.contains("--pretool 'echo hello'"));
            assert!(result.contains("hookSpecificOutput"));
            assert!(result.contains("updatedInput"));
            assert!(!result.contains("LADE_VIA=pretool "));
        },
    );
}

#[test]
fn test_match_wraps_codex() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", None),
            ("CODEX_THREAD_ID", Some("thr_1")),
            ("PI_HOME", None),
            ("OPENCODE", None),
        ],
        || {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"},"hook_event_name":"PreToolUse"}"#;
            let result = handle(&config, input, Audience::Agent).unwrap();
            assert!(result.contains("--pretool 'echo hello'"));
            assert!(result.contains("updatedInput"));
            assert!(!result.contains("LADE_VIA=pretool "));
        },
    );
}

#[test]
fn test_match_wraps_pi() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", None),
            ("CODEX_THREAD_ID", None),
            ("PI_HOME", Some("/tmp/pi")),
            ("OPENCODE", None),
        ],
        || {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_name":"bash","tool_input":{"command":"echo hello"},"hook_event_name":"PreToolUse"}"#;
            let result = handle(&config, input, Audience::Agent).unwrap();
            assert!(result.contains("--pretool 'echo hello'"));
            assert!(result.contains("updatedInput"));
        },
    );
}

#[test]
fn test_claude_no_match_allows_silently() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", Some("/tmp")),
            ("CODEX_THREAD_ID", None),
            ("PI_HOME", None),
            ("OPENCODE", None),
        ],
        || {
            let (config, _dir) = test_config("^terraform");
            let input = r#"{"tool_input":{"command":"echo hello"},"hook_event_name":"PreToolUse"}"#;
            let result = handle(&config, input, Audience::Agent).unwrap();
            assert_eq!(result, "");
        },
    );
}

// Disclaimer enforcement lives in `lade inject` (prints it to stderr, then
// fails closed), so the hook rewrites a disclaimer-carrying command like any
// other match. See `prompt::resolve_disclaimers`.
#[test]
fn test_disclaimer_command_is_rewritten() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", Some("1.0")),
            ("CLAUDE_PROJECT_DIR", None),
            ("CODEX_THREAD_ID", None),
            ("CODEX_SANDBOX", None),
            ("CODEX_HOME", None),
            ("PI_HOME", None),
            ("OPENCODE", None),
            ("LADE_APPROVE", None),
            ("LADE_DISCLAIMER_APPROVED", None),
        ],
        || {
            let (config, _dir) = test_config_with_disclaimer("^echo", "Danger ahead.");
            let input = r#"{"tool_input": {"command": "echo hello"}}"#;
            let result = handle(&config, input, Audience::Agent).unwrap();
            assert!(result.contains("--pretool 'echo hello'"));
            assert!(result.contains("updated_input"));
            assert!(!result.contains("LADE_VIA=pretool "));
            assert!(!result.contains("deny"));
        },
    );
}

#[test]
fn test_env_prefix_kept_before_inject() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("^echo");
        let input = r#"{"tool_input": {"command": "LADE_APPROVE=ab12c echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("LADE_APPROVE=ab12c "));
        assert!(result.contains("--pretool 'echo hello'"));
        assert!(!result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_already_wrapped_stamps_via() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "lade inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("updated_input"));
        assert!(result.contains("lade --pretool inject 'echo'"));
        assert!(!result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_already_wrapped_absolute_path_stamps_via() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/local/bin/lade inject 'echo hello'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("updated_input"));
        assert!(result.contains("/usr/local/bin/lade --pretool inject 'echo hello'"));
        assert!(!result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_already_injected_detects_lade_binaries() {
    assert!(super::platform::is_already_injected("lade --pretool echo"));
    assert!(super::platform::is_already_injected("lade inject 'echo'"));
    assert!(super::platform::is_already_injected(
        "/usr/local/bin/lade inject -- echo"
    ));
    assert!(!super::platform::is_already_injected("lade hook"));
    assert!(!super::platform::is_already_injected("lade status"));
    assert!(!super::platform::is_already_injected("echo lade inject"));
    assert!(super::platform::is_already_injected(
        "/usr/bin/lade.exe inject echo"
    ));
    assert!(super::platform::is_already_injected(
        "LADE_APPROVE=ab12c /usr/bin/lade inject echo"
    ));
}

#[test]
fn test_env_prefix_already_injected_stamps_via() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input =
            r#"{"tool_input": {"command": "LADE_APPROVE=ab12c /usr/bin/lade inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("updated_input"));
        assert!(result.contains("LADE_APPROVE=ab12c "));
        assert!(result.contains("/usr/bin/lade --pretool inject 'echo'"));
        assert!(!result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_hook_stamp_already_injected_skips() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input =
            r#"{"tool_input": {"command": "LADE_VIA=pretool /usr/bin/lade inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_pretool_flag_already_injected_skips() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/bin/lade --pretool 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_via_flag_already_injected_skips() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/bin/lade --via=pretool inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_via_flag_after_inject_already_injected_skips() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/bin/lade inject --via=pretool 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_agent_when_wraps() {
    with_cursor_env(|| {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"^echo\":\n  \".\":\n    when: agent\n  KEY: val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("--pretool 'echo hello'"));
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
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_cursor_version_does_not_override_codex_envelope() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", Some("1.0")),
            ("CLAUDE_PROJECT_DIR", None),
            ("CODEX_THREAD_ID", Some("thr_1")),
            ("PI_HOME", None),
            ("OPENCODE", None),
        ],
        || {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"},"hook_event_name":"PreToolUse","turn_id":"t1"}"#;
            let result = handle(&config, input, Audience::Agent).unwrap();
            assert!(result.contains("updatedInput"));
            assert!(!result.contains("updated_input"));
            assert!(result.contains("--pretool 'echo hello'"));
        },
    );
}

#[test]
fn test_pretool_use_event_keeps_claude_envelope_with_cursor_version() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("^echo");
        let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"},"hook_event_name":"PreToolUse"}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("updatedInput"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn invoked_lade_bin_from_path_stays_bare() {
    use std::ffi::OsString;
    use std::path::PathBuf;

    let exe = PathBuf::from("/opt/lade");
    assert_eq!(
        super::invoked_lade_bin_from(Some(OsString::from("lade")), Some(exe.clone())),
        "lade"
    );
    assert_eq!(
        super::invoked_lade_bin_from(Some(OsString::from("lade.exe")), Some(exe.clone())),
        "lade.exe"
    );
    assert_eq!(
        super::invoked_lade_bin_from(Some(OsString::from("/opt/custom/lade")), Some(exe.clone())),
        "/opt/lade"
    );
    assert_eq!(
        super::invoked_lade_bin_from(Some(OsString::from("./target/debug/lade")), Some(exe)),
        "/opt/lade"
    );
}
