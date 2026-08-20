use super::handle;
use super::platform::{Platform, detect_platform};
use crate::config::{Audience, Config, LadeFile};
use tempfile::{TempDir, tempdir};

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

#[test]
fn test_detect_cursor() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", Some("1.0")),
            ("CLAUDE_PROJECT_DIR", None),
        ],
        || {
            assert_eq!(detect_platform().unwrap(), Platform::Cursor);
        },
    );
}

#[test]
fn test_detect_claude() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", Some("/tmp")),
        ],
        || {
            assert_eq!(detect_platform().unwrap(), Platform::ClaudeCode);
        },
    );
}

#[test]
fn test_detect_unknown_fails() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None::<&str>),
            ("CLAUDE_PROJECT_DIR", None),
        ],
        || {
            assert!(detect_platform().is_err());
        },
    );
}

#[test]
fn test_no_command_allows() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config("echo");
        let result = handle(&config, "{}", Audience::Agent).unwrap();
        assert!(result.contains("allow"));
    });
}

#[test]
fn test_no_match_allows() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config("^terraform");
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
    });
}

#[test]
fn test_match_wraps_cursor() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config("^echo");
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("inject 'echo hello'"));
        assert!(result.contains("updated_input"));
        assert!(result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_match_wraps_claude() {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", None),
            ("CLAUDE_PROJECT_DIR", Some("/tmp")),
        ],
        || {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_input": {"command": "echo hello"}}"#;
            let result = handle(&config, input, Audience::Agent).unwrap();
            assert!(result.contains("inject 'echo hello'"));
            assert!(result.contains("hookSpecificOutput"));
            assert!(result.contains("updatedInput"));
            assert!(result.contains("LADE_VIA=pretool "));
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
            ("LADE_APPROVE", None),
            ("LADE_DISCLAIMER_APPROVED", None),
        ],
        || {
            let (config, _dir) = test_config_with_disclaimer("^echo", "Danger ahead.");
            let input = r#"{"tool_input": {"command": "echo hello"}}"#;
            let result = handle(&config, input, Audience::Agent).unwrap();
            assert!(result.contains("inject 'echo hello'"));
            assert!(result.contains("updated_input"));
            assert!(result.contains("LADE_VIA=pretool "));
            assert!(!result.contains("deny"));
        },
    );
}

#[test]
fn test_env_prefix_kept_before_inject() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config("^echo");
        let input = r#"{"tool_input": {"command": "LADE_APPROVE=ab12c echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("LADE_APPROVE=ab12c "));
        assert!(result.contains("LADE_VIA=pretool "));
        assert!(result.contains("inject 'echo hello'"));
    });
}

#[test]
fn test_already_wrapped_skips() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "lade inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_already_wrapped_absolute_path_skips() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/local/bin/lade inject 'echo hello'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_already_injected_detects_lade_binaries() {
    assert!(super::platform::is_already_injected("lade inject 'echo'"));
    assert!(super::platform::is_already_injected(
        "/usr/local/bin/lade inject -- echo"
    ));
    assert!(!super::platform::is_already_injected("lade hook"));
    assert!(!super::platform::is_already_injected("echo lade inject"));
    assert!(super::platform::is_already_injected(
        "/usr/bin/lade.exe inject echo"
    ));
    assert!(super::platform::is_already_injected(
        "LADE_APPROVE=ab12c /usr/bin/lade inject echo"
    ));
}

#[test]
fn test_env_prefix_already_injected_skips() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config(".*");
        let input =
            r#"{"tool_input": {"command": "LADE_APPROVE=ab12c /usr/bin/lade inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_hook_stamp_already_injected_skips() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config(".*");
        let input =
            r#"{"tool_input": {"command": "LADE_VIA=pretool /usr/bin/lade inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_agent_when_wraps() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"^echo\":\n  \".\":\n    when: agent\n  KEY: val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input, Audience::Agent).unwrap();
        assert!(result.contains("inject 'echo hello'"));
        assert!(result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_human_when_does_not_wrap() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
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
