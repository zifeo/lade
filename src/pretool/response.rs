use super::platform::Platform;
use serde_json::{Value, json};

/// Wrap Claude Code's required `hookSpecificOutput` envelope around `fields`.
fn claude(fields: Value) -> String {
    let mut out = json!({ "hookEventName": "PreToolUse" });
    if let (Some(obj), Some(extra)) = (out.as_object_mut(), fields.as_object()) {
        obj.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    json!({ "hookSpecificOutput": out }).to_string()
}

pub(super) fn format_allow(platform: &Platform) -> String {
    match platform {
        Platform::Cursor => json!({"permission": "allow"}).to_string(),
        Platform::ClaudeCode => String::new(), // exit 0 with no output
    }
}

pub(super) fn format_modify(platform: &Platform, tool_input: &Value, new_command: &str) -> String {
    let mut updated = tool_input.clone();
    updated["command"] = json!(new_command);

    match platform {
        Platform::Cursor => json!({
            "permission": "allow",
            "updated_input": updated
        })
        .to_string(),
        Platform::ClaudeCode => claude(json!({
            "permissionDecision": "allow",
            "updatedInput": updated
        })),
    }
}
