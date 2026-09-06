use serde_json::Value;
use std::env;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Platform {
    Cursor,
    ClaudeCode,
    Codex,
    OpenCode,
}

const CODEX_ENV: [&str; 2] = ["CODEX_THREAD_ID", "CODEX_SANDBOX"];
const OPENCODE_ENV: [&str; 2] = ["OPENCODE", "OPENCODE_DIR"];

impl Platform {
    pub(super) fn slug(self) -> &'static str {
        match self {
            Platform::Cursor => "cursor",
            Platform::ClaudeCode => "claude",
            Platform::Codex => "codex",
            Platform::OpenCode => "opencode",
        }
    }
}

pub(super) fn parse_harness(raw: &str) -> Option<Platform> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "cursor" => Some(Platform::Cursor),
        "claude" | "claude-code" | "claudecode" => Some(Platform::ClaudeCode),
        "codex" => Some(Platform::Codex),
        "opencode" => Some(Platform::OpenCode),
        _ => None,
    }
}

/// Flag first. Unknown flag values are ignored. Detection never fails the hook.
pub(super) fn resolve_platform(harness: Option<&str>, input: &Value) -> Option<Platform> {
    if let Some(platform) = harness.and_then(parse_harness) {
        return Some(platform);
    }
    detect_platform(input).or_else(
        || match input.get("hook_event_name").and_then(Value::as_str) {
            Some("preToolUse" | "beforeMCPExecution") => Some(Platform::Cursor),
            Some("PreToolUse") => Some(Platform::ClaudeCode),
            _ => None,
        },
    )
}

pub(super) fn collect_agent(platform: Option<Platform>, input: &Value) -> Value {
    let mut obj = serde_json::Map::new();
    if let Some(platform) = platform {
        obj.insert("harness".to_string(), serde_json::json!(platform.slug()));
    }
    if let Some(model) = model_from(platform, input) {
        obj.insert("model".to_string(), serde_json::json!(model));
    }
    if let Some(session) = session_from(input) {
        obj.insert("session".to_string(), serde_json::json!(session));
    }
    if obj.is_empty() {
        Value::Null
    } else {
        Value::Object(obj)
    }
}

fn model_from(platform: Option<Platform>, input: &Value) -> Option<String> {
    match platform {
        Some(Platform::Cursor) => crate::agent_meta::pinned(
            input
                .get("model_id")
                .and_then(Value::as_str)
                .or_else(|| input.get("model").and_then(Value::as_str)),
        ),
        Some(Platform::Codex) => {
            crate::agent_meta::pinned(input.get("model").and_then(Value::as_str))
        }
        _ => None,
    }
}

fn session_from(input: &Value) -> Option<String> {
    input
        .get("conversation_id")
        .and_then(Value::as_str)
        .or_else(|| input.get("session_id").and_then(Value::as_str))
        .or_else(|| input.get("sessionID").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Detect the host agent from payload, then hook environment. `CURSOR_VERSION`
/// is last among env signals: Cursor also sets it in other hosts' terminals,
/// and a `PreToolUse` payload must keep the Claude `updatedInput` envelope.
/// `--harness opencode` uses a native `{ command }` rewrite.
pub(super) fn detect_platform(input: &Value) -> Option<Platform> {
    if is_codex(input) {
        return Some(Platform::Codex);
    }
    if is_opencode(input) {
        return Some(Platform::OpenCode);
    }
    if env::var("CLAUDE_PROJECT_DIR").is_ok() || is_pretool_use(input) {
        return Some(Platform::ClaudeCode);
    }
    if env::var("CURSOR_VERSION").is_ok()
        || input.get("hook_event_name").and_then(Value::as_str) == Some("preToolUse")
    {
        return Some(Platform::Cursor);
    }
    None
}

fn is_codex(input: &Value) -> bool {
    CODEX_ENV.iter().any(|key| env::var(key).is_ok()) || is_codex_payload(input)
}

/// `turn_id` is Codex-specific. `model` is not: Cursor now sends it too.
fn is_codex_payload(input: &Value) -> bool {
    is_pretool_use(input) && input.get("turn_id").and_then(Value::as_str).is_some()
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
        .or_else(|| input.get("command").and_then(Value::as_str))
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
pub(crate) fn split_env_prefix(command: &str) -> (String, String) {
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
/// is not enough for a user hook plus a project hook. `--pretool` and
/// `--pretool=<id>` are the alias form.
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
    is_lade
        && parts.any(|part| part == "inject" || is_pretool_flag(part) || part == "--via=pretool")
}

/// True when the rewrite already carries pretool: `--pretool`,
/// `--pretool=<id>`, older `--via=pretool`, or a `LADE_VIA=pretool` env prefix.
pub(super) fn has_pretool_stamp(env_prefix: &str, command: &str) -> bool {
    let stamp = format!(
        "{}={}",
        crate::shell::LADE_VIA,
        crate::audience::Via::PRETOOL
    );
    if env_prefix.split_whitespace().any(|part| part == stamp) {
        return true;
    }
    let parts: Vec<&str> = command.split_whitespace().collect();
    parts
        .iter()
        .any(|part| is_pretool_flag(part) || *part == "--via=pretool")
        || parts
            .windows(2)
            .any(|pair| pair[0] == "--via" && pair[1] == crate::audience::Via::PRETOOL)
}

fn is_pretool_flag(part: &str) -> bool {
    part == "--pretool" || part.starts_with("--pretool=")
}
