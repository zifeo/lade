use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;

use super::agent::Agent;
use super::merge_json::{merge_snapshot_into, snapshot_is_present};

impl Agent {
    pub(super) fn has_hook(self, existing: &str) -> Result<bool> {
        if existing.trim().is_empty() {
            return Ok(false);
        }
        if matches!(self, Agent::OpenCode) {
            return Ok(is_lade_plugin(existing));
        }
        let root = parse_root(existing, self)?;
        let found = match self {
            Agent::Cursor => root
                .pointer("/hooks/preToolUse")
                .and_then(Value::as_array)
                .map(|a| a.iter().any(command_is_lade_hook))
                .unwrap_or(false),
            Agent::Claude | Agent::Codex => root
                .pointer("/hooks/PreToolUse")
                .and_then(Value::as_array)
                .map(|a| a.iter().any(matcher_has_hook))
                .unwrap_or(false),
            Agent::OpenCode => unreachable!("OpenCode returns above"),
        };
        Ok(found)
    }

    pub(super) fn hook_uses_command(self, existing: &str, command: &str) -> Result<bool> {
        if !self.has_hook(existing)? {
            return Ok(false);
        }
        if matches!(self, Agent::OpenCode) {
            return Ok(existing.trim_end() == self.instantiate(command).trim_end());
        }
        let root = parse_root(existing, self)?;
        let snap = parse_root(&self.instantiate(command), self)?;
        Ok(snapshot_is_present(self, &root, &snap))
    }

    pub(super) fn merge(self, existing: &str, command: &str) -> Result<String> {
        let snapshot = self.instantiate(command);
        if matches!(self, Agent::OpenCode) || existing.trim().is_empty() {
            return Ok(snapshot);
        }
        let mut root = parse_root(existing, self)?;
        let snap = parse_root(&snapshot, self)?;
        merge_snapshot_into(self, &mut root, &snap)?;
        to_pretty(&root)
    }

    pub(super) fn remove(self, existing: &str) -> Result<String> {
        let mut root = parse_root(existing, self)?;
        if let Some(obj) = root.as_object_mut() {
            match self {
                Agent::Cursor => {
                    if let Some(arr) = obj
                        .get_mut("hooks")
                        .and_then(|h| h.get_mut("preToolUse"))
                        .and_then(Value::as_array_mut)
                    {
                        arr.retain(|e| !command_is_lade_hook(e));
                    }
                    if let Some(arr) = obj
                        .get_mut("hooks")
                        .and_then(|h| h.get_mut("beforeMCPExecution"))
                        .and_then(Value::as_array_mut)
                    {
                        arr.retain(|e| !command_is_lade_hook(e));
                    }
                }
                Agent::Claude | Agent::Codex | Agent::OpenCode => {
                    // OpenCode here is leftover Claude-compat hooks.json only.
                    if let Some(arr) = obj
                        .get_mut("hooks")
                        .and_then(|h| h.get_mut("PreToolUse"))
                        .and_then(Value::as_array_mut)
                    {
                        for entry in arr.iter_mut() {
                            if let Some(hooks) =
                                entry.get_mut("hooks").and_then(Value::as_array_mut)
                            {
                                hooks.retain(|h| !command_is_lade_hook(h));
                            }
                        }
                        arr.retain(|e| {
                            e.get("hooks")
                                .and_then(Value::as_array)
                                .map(|h| !h.is_empty())
                                .unwrap_or(true)
                        });
                    }
                }
            }
        }
        to_pretty(&root)
    }
}

pub(super) fn is_lade_plugin(content: &str) -> bool {
    content.contains("LadePretool")
        || content.contains("lade hook")
        || content.contains("[\"hook\"]")
        || content.contains("[\"hook\",")
}

/// Recognize `lade hook` and `lade hook --harness cursor`, including an
/// absolute path, so re-running `install` after an upgrade does not duplicate.
pub(super) fn is_lade_hook(command: &str) -> bool {
    let mut parts = command.split_whitespace();
    let prog_is_lade = parts
        .next()
        .and_then(|p| Path::new(p).file_name().and_then(|n| n.to_str()))
        .map(|n| matches!(n, "lade" | "lade.exe"))
        .unwrap_or(false);
    prog_is_lade && parts.next() == Some("hook")
}

pub(super) fn command_is_lade_hook(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .map(is_lade_hook)
        .unwrap_or(false)
}

fn matcher_has_hook(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| hooks.iter().any(command_is_lade_hook))
        .unwrap_or(false)
}

pub(super) fn parse_root(existing: &str, agent: Agent) -> Result<Value> {
    if existing.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(existing)
        .with_context(|| format!("{} config is not valid JSON", agent.name()))
}

fn to_pretty(value: &Value) -> Result<String> {
    // Relies on serde_json's `preserve_order` feature so rewriting a user's
    // config appends our entry without reordering their existing keys.
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}
