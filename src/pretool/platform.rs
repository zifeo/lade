use anyhow::Result;
use serde_json::Value;
use std::env;

#[derive(Debug, PartialEq)]
pub(super) enum Platform {
    Cursor,
    ClaudeCode,
    Codex,
    Pi,
    OpenCode,
}

const CODEX_ENV: [&str; 3] = ["CODEX_THREAD_ID", "CODEX_SANDBOX", "CODEX_HOME"];
const PI_ENV: [&str; 2] = ["PI_HOME", "PI_CODING_AGENT"];
const OPENCODE_ENV: [&str; 2] = ["OPENCODE", "OPENCODE_DIR"];

/// Detect the host agent from payload, then hook environment. `CURSOR_VERSION`
/// is last among env signals: Cursor also sets it in other hosts' terminals,
/// and a `PreToolUse` payload must keep the Claude `updatedInput` envelope.
pub(super) fn detect_platform(input: &Value) -> Result<Platform> {
    if is_codex(input) {
        return Ok(Platform::Codex);
    }
    if is_opencode(input) {
        return Ok(Platform::OpenCode);
    }
    if is_pi(input) {
        return Ok(Platform::Pi);
    }
    if env::var("CLAUDE_PROJECT_DIR").is_ok() || is_pretool_use(input) {
        return Ok(Platform::ClaudeCode);
    }
    if env::var("CURSOR_VERSION").is_ok()
        || input.get("hook_event_name").and_then(Value::as_str) == Some("preToolUse")
    {
        return Ok(Platform::Cursor);
    }
    anyhow::bail!(
        "Unknown platform: none of CURSOR_VERSION, CLAUDE_PROJECT_DIR, \
         CODEX_THREAD_ID, CODEX_SANDBOX, CODEX_HOME, PI_HOME, PI_CODING_AGENT, \
         OPENCODE, or OPENCODE_DIR is set. lade hook supports Cursor, \
         Claude Code, Codex, Pi, and OpenCode."
    )
}

fn is_codex(input: &Value) -> bool {
    CODEX_ENV.iter().any(|key| env::var(key).is_ok()) || is_codex_payload(input)
}

/// Codex documents `turn_id` and `model` as PreToolUse extensions.
fn is_codex_payload(input: &Value) -> bool {
    is_pretool_use(input)
        && (input.get("turn_id").and_then(Value::as_str).is_some()
            || input.get("model").and_then(Value::as_str).is_some())
}

fn is_pi(input: &Value) -> bool {
    PI_ENV.iter().any(|key| env::var(key).is_ok()) || is_pi_payload(input)
}

fn is_pi_payload(input: &Value) -> bool {
    is_pretool_use(input)
        && input
            .get("tool_name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == "bash")
}

fn is_opencode(input: &Value) -> bool {
    OPENCODE_ENV.iter().any(|key| env::var(key).is_ok()) || is_opencode_payload(input)
}

fn is_opencode_payload(input: &Value) -> bool {
    input.get("hook_source").and_then(Value::as_str) == Some("opencode-plugin")
}

fn is_pretool_use(input: &Value) -> bool {
    input.get("hook_event_name").and_then(Value::as_str) == Some("PreToolUse")
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

/// True when a previous `lade hook` already rewrote this into inject.
/// The rewrite may be `lade` or an absolute path, so `starts_with("lade inject")`
/// is not enough for a user hook plus a project hook. `--pretool` is the
/// alias form (`lade --pretool '…'`).
pub(super) fn is_already_injected(command: &str) -> bool {
    let (_, command) = split_env_prefix(command);
    let mut parts = command.split_whitespace();
    let Some(prog) = parts.next() else {
        return false;
    };
    let is_lade = std::path::Path::new(prog)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "lade" | "lade.exe"));
    is_lade && parts.any(|part| part == "inject" || part == "--pretool" || part == "--via=pretool")
}

/// True when the rewrite already carries pretool: `--pretool`, older
/// `--via=pretool`, or a `LADE_VIA=pretool` env prefix.
pub(super) fn has_pretool_stamp(env_prefix: &str, command: &str) -> bool {
    let stamp = format!(
        "{}={}",
        crate::shell::LADE_VIA,
        crate::shell::LADE_VIA_PRETOOL
    );
    if env_prefix.split_whitespace().any(|part| part == stamp) {
        return true;
    }
    let parts: Vec<&str> = command.split_whitespace().collect();
    parts
        .iter()
        .any(|part| *part == "--pretool" || *part == "--via=pretool")
        || parts
            .windows(2)
            .any(|pair| pair[0] == "--via" && pair[1] == crate::shell::LADE_VIA_PRETOOL)
}
