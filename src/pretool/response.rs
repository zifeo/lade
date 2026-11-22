use super::platform::Platform;
use serde_json::{Value, json};

/// Wrap the Claude-compatible `hookSpecificOutput` envelope around `fields`.
/// Codex uses this same PreToolUse rewrite contract.
fn hook_specific(fields: Value) -> String {
    let mut out = json!({ "hookEventName": "PreToolUse" });
    if let (Some(obj), Some(extra)) = (out.as_object_mut(), fields.as_object()) {
        obj.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    json!({ "hookSpecificOutput": out }).to_string()
}

pub(super) fn format_allow(platform: &Platform) -> String {
    match platform {
        Platform::Cursor => json!({"permission": "allow"}).to_string(),
        Platform::ClaudeCode | Platform::Codex | Platform::OpenCode => String::new(),
    }
}

pub(super) fn format_allow_verb(platform: &Platform) -> String {
    match platform {
        Platform::Cursor => json!({"permission": "allow"}).to_string(),
        Platform::ClaudeCode | Platform::Codex | Platform::OpenCode => "{}".to_string(),
    }
}

pub(super) fn format_modify(platform: &Platform, tool_input: &Value, new_command: &str) -> String {
    // § j0my6hfhx.c@sb6xb
    let mut updated = tool_input.clone();
    updated["command"] = json!(new_command);

    match platform {
        Platform::ClaudeCode | Platform::Codex => hook_specific(json!({
            "permissionDecision": "allow",
            "updatedInput": updated
        })),
        Platform::OpenCode => json!({ "command": new_command }).to_string(),
        Platform::Cursor => json!({
            "permission": "allow",
            "updated_input": updated
        })
        .to_string(),
    }
}

pub(super) fn allow(platform: Option<Platform>) -> String {
    // § f7eb9ymg0.b@x86xq
    match platform {
        Some(platform) => format_allow(&platform),
        None => String::new(),
    }
}

pub(super) fn allow_verb(platform: Option<Platform>) -> String {
    // § f7eb9ymg0.b@x3pxr
    match platform {
        Some(platform) => format_allow_verb(&platform),
        None => "{}".to_string(),
    }
}

pub(super) fn modify(platform: Option<Platform>, tool_input: &Value, new_command: &str) -> String {
    // § f7eb9ymg0.b@2q20v
    match platform {
        Some(platform) => format_modify(&platform, tool_input, new_command),
        None => format_modify(&Platform::ClaudeCode, tool_input, new_command),
    }
}

/// OpenCode's plugin answers only with a command, so it gets `failing`, a
/// line that refuses itself.
pub(super) fn deny(
    platform: Option<Platform>,
    tool_input: &Value,
    failing: &str,
    reason: &str,
) -> String {
    // § j0my6hfhx.d@x1g27
    match platform.unwrap_or(Platform::ClaudeCode) {
        Platform::ClaudeCode | Platform::Codex => hook_specific(json!({
            "permissionDecision": "deny",
            "permissionDecisionReason": reason
        })),
        Platform::Cursor => json!({
            "permission": "deny",
            "user_message": reason,
            "agent_message": reason
        })
        .to_string(),
        Platform::OpenCode => format_modify(&Platform::OpenCode, tool_input, failing),
    }
}
