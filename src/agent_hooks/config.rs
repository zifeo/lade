//! Pure, IO-free merge/remove logic for each agent's hook config schema.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub(super) enum Agent {
    Cursor,
    Claude,
}

pub(super) const AGENTS: [Agent; 2] = [Agent::Cursor, Agent::Claude];

impl Agent {
    pub(super) fn name(self) -> &'static str {
        match self {
            Agent::Cursor => "Cursor",
            Agent::Claude => "Claude Code",
        }
    }

    pub(super) fn config_path(self, home: &Path) -> PathBuf {
        match self {
            Agent::Cursor => home.join(".cursor").join("hooks.json"),
            Agent::Claude => home.join(".claude").join("settings.json"),
        }
    }

    pub(super) fn home_dir(self, home: &Path) -> PathBuf {
        match self {
            Agent::Cursor => home.join(".cursor"),
            Agent::Claude => home.join(".claude"),
        }
    }

    pub(super) fn has_hook(self, existing: &str) -> Result<bool> {
        if existing.trim().is_empty() {
            return Ok(false);
        }
        let root = parse_root(existing, self)?;
        let found = match self {
            Agent::Cursor => root
                .pointer("/hooks/preToolUse")
                .and_then(Value::as_array)
                .map(|a| a.iter().any(command_is_lade_hook))
                .unwrap_or(false),
            Agent::Claude => root
                .pointer("/hooks/PreToolUse")
                .and_then(Value::as_array)
                .map(|a| a.iter().any(claude_matcher_has_hook))
                .unwrap_or(false),
        };
        Ok(found)
    }

    pub(super) fn merge(self, existing: &str, command: &str) -> Result<String> {
        let mut root = parse_root(existing, self)?;
        let obj = root
            .as_object_mut()
            .with_context(|| format!("{} config must be a JSON object", self.name()))?;
        match self {
            Agent::Cursor => {
                obj.entry("version").or_insert_with(|| json!(1));
                let arr = obj
                    .entry("hooks")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .context("\"hooks\" must be an object")?
                    .entry("preToolUse")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .context("\"preToolUse\" must be an array")?;
                if !arr.iter().any(command_is_lade_hook) {
                    arr.push(json!({ "command": command, "matcher": "Shell" }));
                }
            }
            Agent::Claude => {
                let arr = obj
                    .entry("hooks")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .context("\"hooks\" must be an object")?
                    .entry("PreToolUse")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .context("\"PreToolUse\" must be an array")?;
                if !arr.iter().any(claude_matcher_has_hook) {
                    arr.push(json!({
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": command }]
                    }));
                }
            }
        }
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
                }
                Agent::Claude => {
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
                        // Drop matcher blocks we emptied, but keep ones the user
                        // authored with a shape we don't recognize.
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

fn claude_matcher_has_hook(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| hooks.iter().any(command_is_lade_hook))
        .unwrap_or(false)
}

fn command_is_lade_hook(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .map(is_lade_hook)
        .unwrap_or(false)
}

/// Recognize both `lade hook` and an absolute path like `/usr/local/bin/lade
/// hook`, so re-running `install` after an upgrade does not duplicate entries.
pub(super) fn is_lade_hook(command: &str) -> bool {
    let mut parts = command.split_whitespace();
    let prog_is_lade = parts
        .next()
        .and_then(|p| Path::new(p).file_name().and_then(|n| n.to_str()))
        .map(|n| matches!(n, "lade" | "lade.exe"))
        .unwrap_or(false);
    prog_is_lade && command.split_whitespace().next_back() == Some("hook")
}

fn parse_root(existing: &str, agent: Agent) -> Result<Value> {
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
