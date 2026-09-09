use super::super::handle;
use super::super::invoked_lade_bin_from;
use super::super::lade_on_path;
use super::{
    assert_wraps_with_ticket, test_config, test_config_with_disclaimer, with_cursor_env,
    with_ticket_tmpdir,
};
use crate::config::{Audience, LadeFile};
use std::ffi::OsString;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_match_wraps_cursor() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_input": {"command": "echo hello"}}"#;
            let result = handle(&config, input, Audience::Agent, None).unwrap();
            assert_wraps_with_ticket(&result, "echo hello");
            assert!(result.contains("updated_input"));
            assert!(!result.contains("LADE_VIA=pretool "));
        });
    });
}

#[test]
fn test_match_wraps_claude() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", Some("/tmp")),
            ("CODEX_THREAD_ID", None),
            ("OPENCODE", None),
        ],
        || {
            with_ticket_tmpdir(|| {
                let (config, _dir) = test_config("^echo");
                let input = r#"{"tool_input": {"command": "echo hello"}}"#;
                let result = handle(&config, input, Audience::Agent, None).unwrap();
                assert_wraps_with_ticket(&result, "echo hello");
                assert!(result.contains("hookSpecificOutput"));
                assert!(result.contains("updatedInput"));
                assert!(!result.contains("LADE_VIA=pretool "));
            });
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
            ("OPENCODE", None),
        ],
        || {
            with_ticket_tmpdir(|| {
                let (config, _dir) = test_config("^echo");
                let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"},"hook_event_name":"PreToolUse"}"#;
                let result = handle(&config, input, Audience::Agent, None).unwrap();
                assert_wraps_with_ticket(&result, "echo hello");
                assert!(result.contains("updatedInput"));
                assert!(!result.contains("LADE_VIA=pretool "));
            });
        },
    );
}

#[test]
fn test_log_only_dot_does_not_wrap() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let dir = tempdir().unwrap();
            std::fs::write(dir.path().join("lade.yml"), ".:\n  .:\n    log: true\n").unwrap();
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let input = r#"{"tool_input": {"command": "gcloud auth list"}}"#;
            let result = handle(&config, input, Audience::Agent, None).unwrap();
            assert!(result.contains("allow"));
            assert!(!result.contains("updated_input"));
            assert!(!result.contains("--pretool"));
            let tickets = std::env::var("LADE_TICKET_DIR").unwrap();
            assert!(
                std::fs::read_dir(&tickets).unwrap().next().is_none(),
                "log-only match must not write a ticket"
            );
        });
    });
}

#[test]
fn test_disclaimer_only_is_rewritten() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let dir = tempdir().unwrap();
            std::fs::write(
                dir.path().join("lade.yml"),
                "\"^echo\":\n  \".\":\n    disclaimer: \"Danger ahead.\"\n",
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
fn test_pin_only_is_rewritten() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let dir = tempdir().unwrap();
            std::fs::write(
                dir.path().join("lade.yml"),
                "\"^jq\":\n  jq: \"core:jq@1.7.1\"\n",
            )
            .unwrap();
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let input = r#"{"tool_input": {"command": "jq --version"}}"#;
            let result = handle(&config, input, Audience::Agent, None).unwrap();
            assert_wraps_with_ticket(&result, "jq --version");
        });
    });
}

#[test]
fn test_claude_no_match_allows_silently() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", Some("/tmp")),
            ("CODEX_THREAD_ID", None),
            ("OPENCODE", None),
        ],
        || {
            let (config, _dir) = test_config("^terraform");
            let input = r#"{"tool_input":{"command":"echo hello"},"hook_event_name":"PreToolUse"}"#;
            let result = handle(&config, input, Audience::Agent, None).unwrap();
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
            ("OPENCODE", None),
            ("LADE_APPROVE", None),
        ],
        || {
            with_ticket_tmpdir(|| {
                let (config, _dir) = test_config_with_disclaimer("^echo", "Danger ahead.");
                let input = r#"{"tool_input": {"command": "echo hello"}}"#;
                let result = handle(&config, input, Audience::Agent, None).unwrap();
                assert_wraps_with_ticket(&result, "echo hello");
                assert!(result.contains("updated_input"));
                assert!(!result.contains("LADE_VIA=pretool "));
                assert!(!result.contains("deny"));
            });
        },
    );
}

#[test]
fn test_env_prefix_kept_before_inject() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_input": {"command": "LADE_APPROVE=ab12c echo hello"}}"#;
            let result = handle(&config, input, Audience::Agent, None).unwrap();
            assert!(result.contains("LADE_APPROVE=ab12c "));
            assert_wraps_with_ticket(&result, "echo hello");
            assert!(!result.contains("LADE_VIA=pretool "));
        });
    });
}

#[test]
fn test_writes_raw_agent_on_ticket() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_name":"Shell","tool_input":{"command":"echo hello"},"hook_event_name":"preToolUse","conversation_id":"conv_1","model_id":"grok"}"#;
            let result = handle(&config, input, Audience::Agent, Some("cursor")).unwrap();
            let after_flag = result
                .split("--pretool")
                .nth(1)
                .expect("result should contain --pretool");
            let token = after_flag
                .strip_prefix('=')
                .unwrap_or(after_flag)
                .split_whitespace()
                .next()
                .expect("token after --pretool");
            let pre: serde_json::Value =
                serde_json::from_slice(&std::fs::read(crate::ticket::path(token)).unwrap())
                    .unwrap();
            assert_eq!(pre["agent"]["harness"], "cursor");
            assert_eq!(pre["agent"]["session"], "conv_1");
            assert_eq!(pre["agent"]["model"], "grok");
        });
    });
}

#[test]
fn test_cursor_version_does_not_override_codex_envelope() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", Some("1.0")),
            ("CLAUDE_PROJECT_DIR", None),
            ("CODEX_THREAD_ID", Some("thr_1")),
            ("OPENCODE", None),
        ],
        || {
            with_ticket_tmpdir(|| {
                let (config, _dir) = test_config("^echo");
                let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"},"hook_event_name":"PreToolUse","turn_id":"t1"}"#;
                let result = handle(&config, input, Audience::Agent, None).unwrap();
                assert!(result.contains("updatedInput"));
                assert!(!result.contains("updated_input"));
                assert_wraps_with_ticket(&result, "echo hello");
            });
        },
    );
}

#[test]
fn test_pretool_use_event_keeps_claude_envelope_with_cursor_version() {
    with_cursor_env(|| {
        let (config, _dir) = test_config("^echo");
        let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"},"hook_event_name":"PreToolUse"}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("updatedInput"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_rewrite_has_ticket_id_between_flag_and_command() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_input": {"command": "echo hello"}}"#;
            let result = handle(&config, input, Audience::Agent, None).unwrap();
            let after_flag = result
                .split("--pretool")
                .nth(1)
                .expect("result should contain --pretool");
            let rest = after_flag.trim_start_matches('=').trim_start();
            let id = rest
                .split_whitespace()
                .next()
                .expect("ticket id after --pretool");
            assert!(crate::ticket::is_id(id));
            assert!(
                rest[id.len()..].trim_start().starts_with("'echo hello'"),
                "quoted command should follow ticket id, got: {rest}"
            );
        });
    });
}

#[test]
fn unread_json_allows_and_stays_open() {
    let (config, _dir) = test_config("^echo");
    let result = handle(&config, "not json at all", Audience::Agent, Some("claude")).unwrap();
    assert_eq!(result, "");
}

#[test]
fn unread_pretool_without_command_allows_and_stays_open() {
    let (config, _dir) = test_config("^echo");
    let result = handle(
        &config,
        r#"{"tool_name":"Bash","hook_event_name":"PreToolUse"}"#,
        Audience::Agent,
        Some("claude"),
    )
    .unwrap();
    assert_eq!(result, "");
}

#[test]
fn invoked_lade_bin_from_path_stays_bare() {
    let exe = PathBuf::from("/opt/lade");
    assert_eq!(
        invoked_lade_bin_from(Some(OsString::from("lade")), Some(exe.clone()), false),
        "lade"
    );
    assert_eq!(
        invoked_lade_bin_from(Some(OsString::from("lade.exe")), Some(exe.clone()), false),
        "lade.exe"
    );
    assert_eq!(
        invoked_lade_bin_from(
            Some(OsString::from("/opt/custom/lade")),
            Some(exe.clone()),
            false
        ),
        "/opt/lade"
    );
    assert_eq!(
        invoked_lade_bin_from(
            Some(OsString::from("./target/debug/lade")),
            Some(exe),
            false
        ),
        "/opt/lade"
    );
}

#[test]
fn invoked_lade_bin_from_path_falls_back_when_on_path() {
    let exe = PathBuf::from("/Users/me/.cargo/bin/lade");
    assert_eq!(
        invoked_lade_bin_from(
            Some(OsString::from("/Users/me/.cargo/bin/lade")),
            Some(exe.clone()),
            true
        ),
        "lade"
    );
    assert_eq!(
        invoked_lade_bin_from(Some(OsString::from("./target/debug/lade")), Some(exe), true),
        "lade"
    );
}

#[test]
fn lade_on_path_sees_a_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lade"), b"").unwrap();
    temp_env::with_var("PATH", Some(dir.path()), || {
        assert!(lade_on_path());
    });
    temp_env::with_var("PATH", Some("/no/such/lade-path"), || {
        assert!(!lade_on_path());
    });
}
