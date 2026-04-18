/*
Agentic tools hook handler for Cursor and Claude Code.

# Cursor Hook Format
- Docs: https://cursor.com/docs/agent/hooks
- Env: `CURSOR_VERSION`, `CURSOR_PROJECT_DIR`
- Input: `{"tool_input": {"command": "..."}}`
- Output (allow): `{"permission": "allow"}`
- Output (modify): `{"permission": "allow", "updated_input": {"command": "..."}}`

# Claude Code Hook Format
- Docs: https://docs.anthropic.com/en/docs/claude-code/hooks
- Env: `CLAUDE_PROJECT_DIR`
- Input: `{"tool_input": {"command": "..."}}`
- Output (allow): exit 0 with no output
- Output (modify): `{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": {"command": "..."}}}`
*/

use crate::config::Config;
use anyhow::Result;
use serde_json::{Value, json};
use std::env;

#[derive(Debug, PartialEq)]
enum Platform {
    Cursor,
    ClaudeCode,
}

fn detect_platform() -> Result<Platform> {
    // Cursor sets CURSOR_VERSION
    // Docs: https://cursor.com/docs/agent/hooks#environment-variables
    if env::var("CURSOR_VERSION").is_ok() {
        return Ok(Platform::Cursor);
    }

    // Claude Code sets CLAUDE_PROJECT_DIR (and CLAUDE_CODE_REMOTE in remote mode)
    // Docs: https://docs.anthropic.com/en/docs/claude-code/hooks#reference-scripts-by-path
    if env::var("CLAUDE_PROJECT_DIR").is_ok() {
        return Ok(Platform::ClaudeCode);
    }

    anyhow::bail!(
        "Unknown platform: neither CURSOR_VERSION nor CLAUDE_PROJECT_DIR is set. \
         lade hook only supports Cursor and Claude Code."
    )
}

fn extract_command(input: &Value) -> Option<String> {
    input
        .get("tool_input")
        .and_then(|ti| ti.get("command"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
}

fn format_allow(platform: &Platform) -> String {
    match platform {
        Platform::Cursor => json!({"permission": "allow"}).to_string(),
        Platform::ClaudeCode => String::new(), // exit 0 with no output
    }
}

fn format_modify(platform: &Platform, tool_input: &Value, new_command: &str) -> String {
    let mut updated = tool_input.clone();
    updated["command"] = json!(new_command);

    match platform {
        Platform::Cursor => json!({
            "permission": "allow",
            "updated_input": updated
        })
        .to_string(),
        Platform::ClaudeCode => json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "updatedInput": updated
            }
        })
        .to_string(),
    }
}

pub fn handle(config: &Config, input: &str) -> Result<String> {
    let platform = detect_platform()?;
    let parsed: Value = serde_json::from_str(input).unwrap_or(json!({}));

    let command = match extract_command(&parsed) {
        Some(cmd) => cmd,
        None => return Ok(format_allow(&platform)),
    };

    // Skip if already wrapped
    if command.starts_with("lade inject") {
        return Ok(format_allow(&platform));
    }

    // Check if any patterns match
    let matches = config.collect(&command);
    if matches.is_empty() {
        return Ok(format_allow(&platform));
    }

    // Get lade binary path
    let lade_bin = env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "lade".to_string());

    // Wrap with lade inject
    let escaped = command.replace('\'', "'\\''");
    let new_command = format!("{} inject '{}'", lade_bin, escaped);

    let tool_input = parsed.get("tool_input").cloned().unwrap_or(json!({}));
    Ok(format_modify(&platform, &tool_input, &new_command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LadeFile;
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

    #[test]
    fn test_already_wrapped_skips() {
        temp_env::with_var("CURSOR_VERSION", Some("1.0"), || {
            let (config, _dir) = test_config(".*");
            let input = r#"{"tool_input": {"command": "lade inject 'echo'"}}"#;
            let result = handle(&config, input).unwrap();
            assert!(result.contains("allow"));
        });
    }
}
