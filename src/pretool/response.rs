use super::platform::Platform;
use serde_json::{Value, json};

/// Wrap the Claude-compatible `hookSpecificOutput` envelope around `fields`.
/// Codex and Pi use this same PreToolUse rewrite contract.
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
        Platform::ClaudeCode | Platform::Codex | Platform::Pi | Platform::OpenCode => String::new(),
    }
}

pub(super) fn format_modify(platform: &Platform, tool_input: &Value, new_command: &str) -> String {
    let mut updated = tool_input.clone();
    updated["command"] = json!(new_command);

    match platform {
        Platform::ClaudeCode | Platform::Codex | Platform::Pi => hook_specific(json!({
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
