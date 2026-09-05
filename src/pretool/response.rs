use super::platform::Platform;
use serde_json::{Value, json};

/// Wrap the Claude-compatible `hookSpecificOutput` envelope around `fields`.
/// Codex, Pi, and OpenCode use this same PreToolUse rewrite contract.
fn hook_specific(fields: Value) -> String {
    let mut out = json!({ "hookEventName": "PreToolUse" });
    if let (Some(obj), Some(extra)) = (out.as_object_mut(), fields.as_object()) {
        obj.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    json!({ "hookSpecificOutput": out }).to_string()
}

fn uses_claude_envelope(platform: &Platform) -> bool {
    matches!(
        platform,
        Platform::ClaudeCode | Platform::Codex | Platform::Pi | Platform::OpenCode
    )
}

pub(super) fn format_allow(platform: &Platform) -> String {
    if uses_claude_envelope(platform) {
        return String::new();
    }
    json!({"permission": "allow"}).to_string()
}

pub(super) fn format_modify(platform: &Platform, tool_input: &Value, new_command: &str) -> String {
    let mut updated = tool_input.clone();
    updated["command"] = json!(new_command);

    if uses_claude_envelope(platform) {
        return hook_specific(json!({
            "permissionDecision": "allow",
            "updatedInput": updated
        }));
    }
    json!({
        "permission": "allow",
        "updated_input": updated
    })
    .to_string()
}
