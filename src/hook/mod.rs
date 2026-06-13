/*
Agentic tools `preToolUse` hook handler for Cursor and Claude Code.

Invocation via `lade hook` means an AI agent is driving the CLI: the agent
itself invokes this hook before running a tool command. We therefore treat the
context as agent + human-in-the-loop available, and there is no need for the
env-based agent detection used on the direct inject path (see src/agent.rs).

The hook is the recommended transparent path: matching commands are rewritten
into `lade inject '...'` so secrets stay out of the model's context window.
Disclaimer enforcement lives entirely in `lade inject`, which prints the
disclaimer to stderr and fails closed when the command is unapproved (see
`prompt::resolve_disclaimers`); the hook rewrites uniformly. A Cursor `deny`
would make the agent treat the command as forbidden rather than as pending a
human approval, so we deliberately let `lade inject` be the single gate.

# Cursor preToolUse — https://cursor.com/docs/agent/hooks (verified June 2026)
- Env: `CURSOR_VERSION`, `CURSOR_PROJECT_DIR`
- Input: `{"tool_name": "Shell", "tool_input": {"command": "..."}, "hook_event_name": "preToolUse", ...}`
- Output: `{"permission": "allow", "updated_input": {...}}`

# Claude Code PreToolUse — https://code.claude.com/docs/en/hooks (verified June 2026)
- Env: `CLAUDE_PROJECT_DIR`
- Input: `{"tool_name": "Bash", "tool_input": {"command": "..."}, "hook_event_name": "PreToolUse", ...}`
- Output: `{"hookSpecificOutput": {"hookEventName": "PreToolUse",
  "permissionDecision": "allow", "updatedInput": {...}}}`
*/

mod platform;
mod response;
#[cfg(test)]
mod tests;

use crate::config::Config;
use anyhow::Result;
use serde_json::{Value, json};
use std::env;

use platform::{detect_platform, extract_command, split_env_prefix};
use response::{format_allow, format_modify};

pub fn handle(config: &Config, input: &str) -> Result<String> {
    let platform = detect_platform()?;
    let parsed: Value = serde_json::from_str(input).unwrap_or(json!({}));

    let raw = match extract_command(&parsed) {
        Some(cmd) => cmd,
        None => return Ok(format_allow(&platform)),
    };

    // Keep any leading `LADE_APPROVE=...` (or other env assignments) so the
    // approval prefix reaches the wrapped `lade inject` process.
    let (env_prefix, command) = split_env_prefix(&raw);

    if command.starts_with("lade inject") {
        return Ok(format_allow(&platform));
    }

    let matches = config.collect(&command);
    if matches.is_empty() {
        return Ok(format_allow(&platform));
    }

    let lade_bin = env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "lade".to_string());
    let escaped = command.replace('\'', "'\\''");
    let new_command = if env_prefix.is_empty() {
        format!("{} inject '{}'", lade_bin, escaped)
    } else {
        format!("{} {} inject '{}'", env_prefix, lade_bin, escaped)
    };
    let tool_input = parsed.get("tool_input").cloned().unwrap_or(json!({}));

    Ok(format_modify(&platform, &tool_input, &new_command))
}
