//! Pure, IO-free merge/remove logic for each agent's hook config schema.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub(super) enum Agent {
    Cursor,
    Claude,
    Codex,
    OpenCode,
}

pub(super) const AGENTS: [Agent; 4] = [Agent::Cursor, Agent::Claude, Agent::Codex, Agent::OpenCode];

impl Agent {
    pub(super) fn name(self) -> &'static str {
        match self {
            Agent::Cursor => "Cursor",
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex",
            Agent::OpenCode => "OpenCode",
        }
    }

    pub(super) fn slug(self) -> &'static str {
        match self {
            Agent::Cursor => "cursor",
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::OpenCode => "opencode",
        }
    }

    pub(super) fn config_path(self, home: &Path) -> PathBuf {
        match self {
            Agent::Cursor => home.join(".cursor").join("hooks.json"),
            Agent::Claude => home.join(".claude").join("settings.json"),
            Agent::Codex => home.join(".codex").join("hooks.json"),
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
            Agent::OpenCode => home.join(".config").join("opencode"),
        }
    }

    pub(super) fn skill_path(self, home: &Path) -> PathBuf {
        self.home_dir(home)
            .join("skills")
            .join("lade")
            .join("SKILL.md")
    }

    fn shell_matcher(self) -> &'static str {
        "Bash"
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
            Agent::Claude | Agent::Codex => root
                .pointer("/hooks/PreToolUse")
                .and_then(Value::as_array)
                .map(|a| a.iter().any(matcher_has_hook))
                .unwrap_or(false),
            Agent::OpenCode => false,
        };
        Ok(found)
    }

    pub(super) fn hook_uses_command(self, existing: &str, command: &str) -> Result<bool> {
        self.hook_uses_command_scoped(existing, command, false)
    }

    pub(super) fn hook_uses_command_scoped(
        self,
        existing: &str,
        command: &str,
        project: bool,
    ) -> Result<bool> {
        if !self.has_hook(existing)? {
            return Ok(false);
        }
        if matches!(self, Agent::OpenCode) {
            return Ok(existing.trim_end() == opencode_plugin_body(command).trim_end());
        }
        let root = parse_root(existing, self)?;
        Ok(match self {
            Agent::Cursor => {
                cursor_has_matcher(&root, "Shell", command)
                    && cursor_has_matcher(&root, "MCP:", command)
                    && (project || cursor_has_before_mcp(&root, command))
            }
            Agent::Claude | Agent::Codex => {
                claude_has_matcher(&root, "Bash", command)
                    && claude_has_matcher(&root, "mcp__.*", command)
            }
            Agent::OpenCode => false,
        })
    }

    pub(super) fn merge(self, existing: &str, command: &str) -> Result<String> {
        self.merge_scoped(existing, command, false)
    }

    pub(super) fn merge_scoped(
        self,
        existing: &str,
        command: &str,
        project: bool,
    ) -> Result<String> {
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
                upsert_cursor_matcher(arr, command, "Shell");
                upsert_cursor_matcher(arr, command, "MCP:");
                if !project {
                    let before = hooks
                        .entry("beforeMCPExecution")
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                        .context("\"beforeMCPExecution\" must be an array")?;
                    upsert_cursor_before_mcp(before, command);
                }
            }
            Agent::Claude | Agent::Codex => {
                let arr = obj
                    .entry("hooks")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .context("\"hooks\" must be an object")?
                    .entry("PreToolUse")
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .context("\"PreToolUse\" must be an array")?;
                upsert_claude_matcher(arr, command, self.shell_matcher());
                upsert_claude_matcher(arr, command, "mcp__.*");
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

fn cursor_has_before_mcp(root: &Value, command: &str) -> bool {
    root.pointer("/hooks/beforeMCPExecution")
        .and_then(Value::as_array)
        .map(|entries| {
            entries.iter().any(|entry| {
                command_is_lade_hook(entry)
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

fn upsert_cursor_before_mcp(arr: &mut Vec<Value>, command: &str) {
    if let Some(entry) = arr.iter_mut().find(|entry| command_is_lade_hook(entry)) {
        entry["command"] = json!(command);
        return;
    }
    arr.push(json!({ "command": command }));
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
const local = new Set(["bash", "read", "write", "grep", "toolsearch", "edit", "glob", "list"]);

// OpenCode loads every exported function in this file and calls it for hooks.
export const LadePretool = async () => ({{
  "tool.execute.before": async (input, output) => {{
    const tool = input.tool;
    if (tool === "bash") {{
      const command = output.args?.command;
      if (typeof command !== "string") {{
        return;
      }}
      const result = spawnSync(lade, ["hook", "--harness", "opencode"], {{
        input: JSON.stringify({{ command, session_id: input.sessionID }}),
        encoding: "utf8",
      }});
      if (result.status !== 0 || !result.stdout?.trim()) {{
        return;
      }}
      try {{
        const updated = JSON.parse(result.stdout)?.command;
        if (typeof updated === "string") {{
          output.args.command = updated;
        }}
      }} catch {{}}
      return;
    }}
    if (typeof tool !== "string" || local.has(tool.toLowerCase())) {{
      return;
    }}
    spawnSync(lade, ["hook", "--harness", "opencode"], {{
      input: JSON.stringify({{
        tool,
        args: output.args ?? {{}},
        session_id: input.sessionID,
        hook_source: "opencode-plugin",
      }}),
      encoding: "utf8",
    }});
  }},
}});
"#
    )
}
