/*
preToolUse handler for Cursor and Claude Code (`lade hook`).

`detect()` classifies this process as Via::Pretool / Audience::Agent. Matching
commands are rewritten into `LADE_VIA=pretool … inject '…'` so the child inject
keeps that classification. Disclaimer enforcement lives in `lade inject`.

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

pub mod install;
mod platform;
mod response;
#[cfg(test)]
mod tests;

use crate::config::{Audience, Config};
use anyhow::Result;
use serde_json::{Value, json};
use std::env;

use platform::{detect_platform, extract_command, is_already_injected, split_env_prefix};
use response::{format_allow, format_modify};

pub fn handle(config: &Config, input: &str, audience: Audience) -> Result<String> {
    let platform = detect_platform()?;
    let parsed: Value = serde_json::from_str(input).unwrap_or(json!({}));

    let raw = match extract_command(&parsed) {
        Some(cmd) => cmd,
        None => return Ok(format_allow(&platform)),
    };

    // Keep any leading `LADE_APPROVE=...` (or other env assignments) so the
    // approval prefix reaches the wrapped `lade inject` process.
    let (env_prefix, command) = split_env_prefix(&raw);

    if is_already_injected(&command) {
        return Ok(format_allow(&platform));
    }

    let matches = config.collect_for(&command, audience);
    if matches.is_empty() {
        return Ok(format_allow(&platform));
    }

    let lade_bin = env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "lade".to_string());
    let escaped = command.replace('\'', "'\\''");
    let stamp = format!(
        "{}={}",
        crate::shell::LADE_VIA,
        crate::shell::LADE_VIA_PRETOOL
    );
    let new_command = if env_prefix.is_empty() {
        format!("{} {} inject '{}'", stamp, lade_bin, escaped)
    } else {
        format!("{} {} {} inject '{}'", env_prefix, stamp, lade_bin, escaped)
    };
    let tool_input = parsed.get("tool_input").cloned().unwrap_or(json!({}));

    Ok(format_modify(&platform, &tool_input, &new_command))
}
