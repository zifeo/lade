//! Pure, IO-free merge/remove logic for each agent's hook config schema.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub(super) enum Agent {
    Cursor,
    Claude,
    Codex,
    Pi,
    OpenCode,
}

pub(super) const AGENTS: [Agent; 5] = [
    Agent::Cursor,
    Agent::Claude,
    Agent::Codex,
    Agent::Pi,
    Agent::OpenCode,
];

impl Agent {
    pub(super) fn name(self) -> &'static str {
        match self {
            Agent::Cursor => "Cursor",
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex",
            Agent::Pi => "Pi",
            Agent::OpenCode => "OpenCode",
        }
    }

    pub(super) fn config_path(self, home: &Path) -> PathBuf {
        match self {
            Agent::Cursor => home.join(".cursor").join("hooks.json"),
            Agent::Claude => home.join(".claude").join("settings.json"),
            Agent::Codex => home.join(".codex").join("hooks.json"),
            Agent::Pi => home.join(".pi").join("agent").join("settings.json"),
            Agent::OpenCode => home
                .join(".config")
                .join("opencode")
                .join("plugins")
                .join("lade-pretool.js"),
        }
    }

    pub(super) fn home_dir(self, home: &Path) -> PathBuf {
        match self {
            Agent::Cursor => home.join(".cursor"),
            Agent::Claude => home.join(".claude"),
            Agent::Codex => home.join(".codex"),
            Agent::Pi => home.join(".pi"),
            Agent::OpenCode => home.join(".config").join("opencode"),
        }
    }

    fn shell_matcher(self) -> &'static str {
        match self {
            Agent::Pi => "Bash|bash",
            _ => "Bash",
        }
    }

    /// Claude-compat `hooks.json` left by an older install. Native OpenCode
    /// ignores it; uninstall still strips our entry.
    pub(super) fn legacy_json_path(self, home: &Path) -> Option<PathBuf> {
        match self {
            Agent::OpenCode => Some(home.join(".config").join("opencode").join("hooks.json")),
            _ => None,
        }
    }

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
            Agent::Claude | Agent::Codex | Agent::Pi => root
                .pointer("/hooks/PreToolUse")
                .and_then(Value::as_array)
                .map(|a| a.iter().any(matcher_has_hook))
                .unwrap_or(false),
            Agent::OpenCode => false,
        };
        Ok(found)
    }

    pub(super) fn hook_uses_command(self, existing: &str, command: &str) -> Result<bool> {
        if !self.has_hook(existing)? {
            return Ok(false);
        }
        if matches!(self, Agent::OpenCode) {
            return Ok(existing.trim_end() == opencode_plugin_body(command).trim_end());
        }
        let root = parse_root(existing, self)?;
        Ok(match self {
            Agent::Cursor => root
                .pointer("/hooks/preToolUse")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries.iter().any(|entry| {
                        command_is_lade_hook(entry)
                            && entry.get("command").and_then(Value::as_str) == Some(command)
                    })
                })
                .unwrap_or(false),
            Agent::Claude | Agent::Codex | Agent::Pi => root
                .pointer("/hooks/PreToolUse")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries.iter().any(|entry| {
                        entry
                            .get("hooks")
                            .and_then(Value::as_array)
                            .map(|hooks| {
                                hooks.iter().any(|hook| {
                                    command_is_lade_hook(hook)
                                        && hook.get("command").and_then(Value::as_str)
                                            == Some(command)
                                })
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false),
            Agent::OpenCode => false,
        })
    }

    pub(super) fn merge(self, existing: &str, command: &str) -> Result<String> {
        if matches!(self, Agent::OpenCode) {
            return Ok(opencode_plugin_body(command));
        }
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
                if let Some(entry) = arr.iter_mut().find(|e| command_is_lade_hook(e)) {
                    entry["command"] = json!(command);
                } else {
                    arr.push(json!({ "command": command, "matcher": "Shell" }));
                }
            }
            Agent::Claude | Agent::Codex | Agent::Pi => {
                let matcher = self.shell_matcher();
                let arr = obj
                    .entry("hooks")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .context("\"hooks\" must be an object")?
                    .entry("PreToolUse")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .context("\"PreToolUse\" must be an array")?;
                if let Some(entry) = arr.iter_mut().find(|e| matcher_has_hook(e)) {
                    if let Some(hooks) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                        for hook in hooks.iter_mut() {
                            if command_is_lade_hook(hook) {
                                hook["command"] = json!(command);
                            }
                        }
                    }
                } else {
                    arr.push(json!({
                        "matcher": matcher,
                        "hooks": [{ "type": "command", "command": command }]
                    }));
                }
            }
            Agent::OpenCode => {}
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
                Agent::Claude | Agent::Codex | Agent::Pi | Agent::OpenCode => {
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

fn matcher_has_hook(entry: &Value) -> bool {
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

pub(super) fn is_lade_plugin(content: &str) -> bool {
    content.contains("lade hook") || content.contains("[\"hook\"]")
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

fn opencode_plugin_body(command: &str) -> String {
    let bin = command
        .split_whitespace()
        .next()
        .unwrap_or("lade")
        .replace('\\', "\\\\")
        .replace('\'', "\\'");
    format!(
        r#"import {{ spawnSync }} from "node:child_process";

const lade = process.env.LADE_BIN ?? "{bin}";

// OpenCode loads every exported function in this file and calls it for hooks.
export const LadePretool = async () => ({{
  "tool.execute.before": async (input, output) => {{
    const command = output.args?.command;
    if (input.tool !== "bash" || typeof command !== "string") {{
      return;
    }}
    const payload = JSON.stringify({{
      hook_event_name: "PreToolUse",
      tool_name: "Bash",
      tool_input: {{ command }},
      hook_source: "opencode-plugin",
    }});
    const result = spawnSync(lade, ["hook"], {{
      input: payload,
      encoding: "utf8",
      env: {{ ...process.env, OPENCODE: "1" }},
    }});
    if (result.status !== 0 || !result.stdout?.trim()) {{
      return;
    }}
    let parsed;
    try {{
      parsed = JSON.parse(result.stdout);
    }} catch {{
      return;
    }}
    const updated = parsed?.hookSpecificOutput?.updatedInput?.command;
    if (typeof updated === "string") {{
      output.args.command = updated;
    }}
  }},
}});
"#
    )
}
