use serde_json::Value;
use std::env;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Platform {
    // § mqnfssecp.c@7gf6d
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
    // § mqnfssecp.c@xk7fx
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
    // § f7eb9ymg0.b@rqdw4
    // § one j0my6hfhx.a@8rvek
    if let Some(platform) = harness.and_then(parse_harness) {
        return Some(platform);
    }
    detect_platform(input)
}

pub(super) fn collect_agent(platform: Option<Platform>, input: &Value) -> Value {
    // § j1mbwadzc.e@2r5kn
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

/// Detect the host agent from the payload's event, then the hook environment.
/// A harness started from another harness's terminal inherits its variables,
/// so an env signal names the harness only for a payload whose event names
/// none. `CURSOR_VERSION` is last: Cursor also sets it in other hosts'
/// terminals.
pub(super) fn detect_platform(input: &Value) -> Option<Platform> {
    // § j0my6hfhx.b@fpne7
    payload_platform(input).or_else(env_platform)
}

/// `turn_id` is Codex-specific. `model` is not: Cursor now sends it too.
fn payload_platform(input: &Value) -> Option<Platform> {
    // § mqnfssecp.c@gx56v
    if is_opencode_payload(input) {
        return Some(Platform::OpenCode);
    }
    match input.get("hook_event_name").and_then(Value::as_str) {
        Some("PreToolUse") if input.get("turn_id").and_then(Value::as_str).is_some() => {
            Some(Platform::Codex)
        }
        Some("PreToolUse") => Some(Platform::ClaudeCode),
        Some("preToolUse" | "beforeMCPExecution") => Some(Platform::Cursor),
        _ => None,
    }
    // § end mqnfssecp.c
}

fn env_platform() -> Option<Platform> {
    // § mqnfssecp.c@bas3y
    let set = |key: &str| env::var(key).is_ok();
    if CODEX_ENV.iter().any(|key| set(key)) {
        Some(Platform::Codex)
    } else if OPENCODE_ENV.iter().any(|key| set(key)) {
        Some(Platform::OpenCode)
    } else if set("CLAUDE_PROJECT_DIR") {
        Some(Platform::ClaudeCode)
    } else if set("CURSOR_VERSION") {
        Some(Platform::Cursor)
    } else {
        None
    }
}

fn is_opencode_payload(input: &Value) -> bool {
    input.get("hook_source").and_then(Value::as_str) == Some("opencode-plugin")
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

/// The leading `NAME=value` words the hook may move in front of the
/// rewrite. A value with quotes, expansion, or an operator may hold
/// spaces (`d=$(getconf X)`), so splitting on them would cut a word in two
/// and break the line: it stays inside the wrap instead.
pub(super) fn split_plain_env_prefix(command: &str) -> (String, String) {
    // § hk9h1sqyt.c@wjtph
    let (prefix, rest) = split_prefix(command, |token| {
        is_env_assignment(token) && !token.contains(NOT_PLAIN_VALUE)
    });
    // Before an operator or a line break, the words set shell variables
    // and prefix no command (`T=x && cd $T`): moved out, they would leave
    // the operator at the start of the wrap.
    let head = &command[..command.len() - rest.len()];
    if head.contains('\n') || rest.starts_with(['&', '|', ';', ')']) {
        return (String::new(), command.trim_start().to_string());
    }
    (prefix, rest)
}

const NOT_PLAIN_VALUE: &[char] = &[
    '\'', '"', '$', '`', '\\', '(', ')', '{', '}', ';', '&', '|', '<', '>', '#',
];

fn split_prefix(command: &str, take: impl Fn(&str) -> bool) -> (String, String) {
    let mut prefix: Vec<&str> = Vec::new();
    let mut rest = command.trim_start();
    while let Some((head, tail)) = rest.split_once(char::is_whitespace) {
        if take(head) {
            prefix.push(head);
            rest = tail.trim_start();
        } else {
            break;
        }
    }
    (prefix.join(" "), rest.to_string())
}
