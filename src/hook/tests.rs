use super::handle;
use super::platform::{Platform, detect_platform};
use crate::config::{Config, LadeFile};
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
        let result = handle(&config, "{}").unwrap();
        assert!(result.contains("allow"));
    });
}

#[test]
fn test_no_match_allows() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config("^terraform");
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input).unwrap();
        assert!(result.contains("allow"));
    });
}

#[test]
fn test_match_wraps_cursor() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config("^echo");
        let input = r#"{"tool_input": {"command": "echo hello"}}"#;
        let result = handle(&config, input).unwrap();
        assert!(result.contains("inject 'echo hello'"));
        assert!(result.contains("updated_input"));
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
            let result = handle(&config, input).unwrap();
            assert!(result.contains("inject 'echo hello'"));
            assert!(result.contains("hookSpecificOutput"));
            assert!(result.contains("updatedInput"));
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
            let result = handle(&config, input).unwrap();
            assert!(result.contains("inject 'echo hello'"));
            assert!(result.contains("updated_input"));
            assert!(!result.contains("deny"));
        },
    );
}

#[test]
fn test_env_prefix_kept_before_inject() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config("^echo");
        let input = r#"{"tool_input": {"command": "LADE_APPROVE=ab12c echo hello"}}"#;
        let result = handle(&config, input).unwrap();
        assert!(result.contains("LADE_APPROVE=ab12c "));
        assert!(result.contains("inject 'echo hello'"));
    });
}

#[test]
fn test_already_wrapped_skips() {
    temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "lade inject 'echo'"}}"#;
        let result = handle(&config, input).unwrap();
        assert!(result.contains("allow"));
    });
}
