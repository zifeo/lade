use anyhow::Result;
use serde_json::Value;
use std::env;

#[derive(Debug, PartialEq)]
pub(super) enum Platform {
    Cursor,
    ClaudeCode,
}

/// Detect the host agent from its hook environment. Cursor sets `CURSOR_VERSION`
/// (<https://cursor.com/docs/agent/hooks#environment-variables>); Claude Code
/// sets `CLAUDE_PROJECT_DIR` (<https://code.claude.com/docs/en/hooks>).
pub(super) fn detect_platform() -> Result<Platform> {
    if env::var("CURSOR_VERSION").is_ok() {
        return Ok(Platform::Cursor);
    }
    if env::var("CLAUDE_PROJECT_DIR").is_ok() {
        return Ok(Platform::ClaudeCode);
    }
    anyhow::bail!(
        "Unknown platform: neither CURSOR_VERSION nor CLAUDE_PROJECT_DIR is set. \
         lade hook only supports Cursor and Claude Code."
    )
}

pub(super) fn extract_command(input: &Value) -> Option<String> {
    input
        .get("tool_input")
        .and_then(|ti| ti.get("command"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
}

fn is_env_assignment(token: &str) -> bool {
    match token.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && name.chars().enumerate().all(|(i, c)| {
                    if i == 0 {
                        c.is_ascii_alphabetic() || c == '_'
                    } else {
                        c.is_ascii_alphanumeric() || c == '_'
                    }
                })
        }
        None => false,
    }
}

/// Split leading `VAR=value` assignments (e.g. `LADE_APPROVE=ab12c`) from the
/// rest of the command. The hook re-emits them before `lade inject` so an
/// approval prefix lands in the wrapped process's environment instead of being
/// swallowed into the quoted inject argument.
pub(super) fn split_env_prefix(command: &str) -> (String, String) {
    let mut prefix: Vec<&str> = Vec::new();
    let mut rest = command.trim_start();
    while let Some((head, tail)) = rest.split_once(char::is_whitespace) {
        if is_env_assignment(head) {
            prefix.push(head);
            rest = tail.trim_start();
        } else {
            break;
        }
    }
    (prefix.join(" "), rest.to_string())
}
