use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::agent::Agent;
use super::merge::command_is_lade_hook;

pub(super) fn snapshot_is_present(agent: Agent, root: &Value, snap: &Value) -> bool {
    match agent {
        Agent::Cursor => snap
            .pointer("/hooks/preToolUse")
            .and_then(Value::as_array)
            .map(|entries| {
                entries.iter().all(|entry| {
                    let Some(matcher) = entry.get("matcher").and_then(Value::as_str) else {
                        return false;
                    };
                    let Some(command) = entry.get("command").and_then(Value::as_str) else {
                        return false;
                    };
                    cursor_has_matcher(root, matcher, command)
                })
            })
            .unwrap_or(false),
        Agent::Claude | Agent::Codex => snap
            .pointer("/hooks/PreToolUse")
            .and_then(Value::as_array)
            .map(|entries| {
                entries.iter().all(|entry| {
                    let Some(matcher) = entry.get("matcher").and_then(Value::as_str) else {
                        return false;
                    };
                    let Some(command) = entry.pointer("/hooks/0/command").and_then(Value::as_str)
                    else {
                        return false;
                    };
                    claude_has_matcher(root, matcher, command)
                })
            })
            .unwrap_or(false),
        Agent::OpenCode => false,
    }
}

pub(super) fn merge_snapshot_into(agent: Agent, root: &mut Value, snap: &Value) -> Result<()> {
    let obj = root
        .as_object_mut()
        .with_context(|| format!("{} config must be a JSON object", agent.name()))?;
    if let Some(version) = snap.get("version") {
        obj.entry("version").or_insert_with(|| version.clone());
    }
    match agent {
        Agent::Cursor => merge_cursor(obj, snap)?,
        Agent::Claude | Agent::Codex => merge_claude(obj, snap)?,
        Agent::OpenCode => {}
    }
    Ok(())
}

fn merge_cursor(obj: &mut serde_json::Map<String, Value>, snap: &Value) -> Result<()> {
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("\"hooks\" must be an object")?;
    let arr = hooks
        .entry("preToolUse")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("\"preToolUse\" must be an array")?;
    if let Some(entries) = snap.pointer("/hooks/preToolUse").and_then(Value::as_array) {
        for entry in entries {
            let matcher = entry
                .get("matcher")
                .and_then(Value::as_str)
                .context("snapshot preToolUse entry needs matcher")?;
            let command = entry
                .get("command")
                .and_then(Value::as_str)
                .context("snapshot preToolUse entry needs command")?;
            upsert_cursor_matcher(arr, command, matcher);
        }
    }
    Ok(())
}

fn merge_claude(obj: &mut serde_json::Map<String, Value>, snap: &Value) -> Result<()> {
    let arr = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("\"hooks\" must be an object")?
        .entry("PreToolUse")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("\"PreToolUse\" must be an array")?;
    if let Some(entries) = snap.pointer("/hooks/PreToolUse").and_then(Value::as_array) {
        for entry in entries {
            let matcher = entry
                .get("matcher")
                .and_then(Value::as_str)
                .context("snapshot PreToolUse entry needs matcher")?;
            let command = entry
                .pointer("/hooks/0/command")
                .and_then(Value::as_str)
                .context("snapshot PreToolUse entry needs command")?;
            upsert_claude_matcher(arr, command, matcher);
        }
    }
    Ok(())
}

fn cursor_has_matcher(root: &Value, matcher: &str, command: &str) -> bool {
    root.pointer("/hooks/preToolUse")
        .and_then(Value::as_array)
        .map(|entries| {
            entries.iter().any(|entry| {
                command_is_lade_hook(entry)
                    && entry.get("matcher").and_then(Value::as_str) == Some(matcher)
                    && entry.get("command").and_then(Value::as_str) == Some(command)
            })
        })
        .unwrap_or(false)
}

fn claude_has_matcher(root: &Value, matcher: &str, command: &str) -> bool {
    root.pointer("/hooks/PreToolUse")
        .and_then(Value::as_array)
        .map(|entries| {
            entries.iter().any(|entry| {
                entry.get("matcher").and_then(Value::as_str) == Some(matcher)
                    && entry
                        .get("hooks")
                        .and_then(Value::as_array)
                        .map(|hooks| {
                            hooks.iter().any(|hook| {
                                command_is_lade_hook(hook)
                                    && hook.get("command").and_then(Value::as_str) == Some(command)
                            })
                        })
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn upsert_cursor_matcher(arr: &mut Vec<Value>, command: &str, matcher: &str) {
    if let Some(entry) = arr.iter_mut().find(|entry| {
        command_is_lade_hook(entry) && entry.get("matcher").and_then(Value::as_str) == Some(matcher)
    }) {
        entry["command"] = json!(command);
        return;
    }
    arr.push(json!({ "command": command, "matcher": matcher }));
}

fn upsert_claude_matcher(arr: &mut Vec<Value>, command: &str, matcher: &str) {
    if let Some(entry) = arr
        .iter_mut()
        .find(|entry| entry.get("matcher").and_then(Value::as_str) == Some(matcher))
    {
        if let Some(hooks) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
            if let Some(hook) = hooks.iter_mut().find(|hook| command_is_lade_hook(hook)) {
                hook["command"] = json!(command);
                return;
            }
            hooks.push(json!({ "type": "command", "command": command }));
            return;
        }
        entry["hooks"] = json!([{ "type": "command", "command": command }]);
        return;
    }
    arr.push(json!({
        "matcher": matcher,
        "hooks": [{ "type": "command", "command": command }]
    }));
}
