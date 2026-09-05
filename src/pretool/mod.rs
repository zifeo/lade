/*
preToolUse handler for Cursor, Claude Code, and Codex (`lade hook`).

`detect()` classifies this process as Via::Pretool / Audience::Agent. Matching
commands are rewritten into `<lade> --pretool '…'` (the inject alias) so the
child keeps that classification and gets `LADE_VIA=pretool` in its env.
Disclaimer enforcement lives in inject.

# Cursor preToolUse — https://cursor.com/docs/agent/hooks (verified June 2026)
- Env: `CURSOR_VERSION`, `CURSOR_PROJECT_DIR`
- Input: `{"tool_name": "Shell", "tool_input": {"command": "..."}, "hook_event_name": "preToolUse", ...}`
- Output: `{"permission": "allow", "updated_input": {...}}`

# Claude-compatible PreToolUse (Claude Code, Codex, Pi, OpenCode)
- Claude: `CLAUDE_PROJECT_DIR` — https://code.claude.com/docs/en/hooks
- Codex: `CODEX_THREAD_ID` / `CODEX_SANDBOX` / `CODEX_HOME`, plus `turn_id`/`model`
  — https://developers.openai.com/codex/hooks
- Pi: `PI_HOME` / `PI_CODING_AGENT`, payload `tool_name` is often `bash`
- OpenCode: `OPENCODE` / `OPENCODE_DIR`, or `hook_source=opencode-plugin`
- Input: `{"tool_name": "Bash"|"bash", "tool_input": {"command": "..."},
  "hook_event_name": "PreToolUse", ...}`
- Output: `{"hookSpecificOutput": {"hookEventName": "PreToolUse",
  "permissionDecision": "allow", "updatedInput": {...}}}`. Exit 0 with no
  stdout allows the original command. Shell tools match as `Bash` (or `bash`
  on Pi).
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
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use platform::{
    detect_platform, extract_command, has_pretool_stamp, is_already_injected, split_env_prefix,
};
use response::{format_allow, format_modify};

fn pretool_flag() -> &'static str {
    "--pretool"
}

/// Bin name for hook install and match rewrites.
/// `lade install` / `lade hook` stay `lade`. A path in argv[0] uses current_exe.
pub(crate) fn invoked_lade_bin() -> String {
    invoked_lade_bin_from(env::args_os().next(), env::current_exe().ok())
}

pub(crate) fn invoked_lade_bin_from(
    argv0: Option<OsString>,
    current_exe: Option<PathBuf>,
) -> String {
    let argv0 = argv0.unwrap_or_default();
    let path = Path::new(&argv0);
    if !argv0.is_empty() && path.file_name() == Some(path.as_os_str()) {
        return path.to_str().unwrap_or("lade").to_string();
    }
    current_exe
        .and_then(|p| p.to_str().map(str::to_string))
        .or_else(|| argv0.to_str().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "lade".to_string())
}

pub fn handle(config: &Config, input: &str, audience: Audience) -> Result<String> {
    let parsed: Value = serde_json::from_str(input).unwrap_or(json!({}));
    let platform = detect_platform(&parsed)?;

    let raw = match extract_command(&parsed) {
        Some(cmd) => cmd,
        None => return Ok(format_allow(&platform)),
    };

    // Keep any leading `LADE_APPROVE=...` (or other env assignments) so the
    // approval prefix reaches the wrapped process.
    let (env_prefix, command) = split_env_prefix(&raw);
    let tool_input = parsed.get("tool_input").cloned().unwrap_or(json!({}));

    if is_already_injected(&command) {
        if has_pretool_stamp(&env_prefix, &command) {
            return Ok(format_allow(&platform));
        }
        let stamped = insert_pretool_flag(&command);
        let rewritten = if env_prefix.is_empty() {
            stamped
        } else {
            format!("{} {}", env_prefix, stamped)
        };
        return Ok(format_modify(&platform, &tool_input, &rewritten));
    }

    let matches = config.collect_for(&command, audience);
    if matches.is_empty() {
        return Ok(format_allow(&platform));
    }

    let lade_bin = invoked_lade_bin();
    let escaped = command.replace('\'', "'\\''");
    let wrapped = format!("{} {} '{}'", lade_bin, pretool_flag(), escaped);
    let new_command = if env_prefix.is_empty() {
        wrapped
    } else {
        format!("{} {}", env_prefix, wrapped)
    };
    Ok(format_modify(&platform, &tool_input, &new_command))
}

fn insert_pretool_flag(command: &str) -> String {
    let mut parts = command.splitn(2, char::is_whitespace);
    let Some(bin) = parts.next() else {
        return command.to_string();
    };
    let rest = parts.next().unwrap_or("");
    if rest.is_empty() {
        format!("{} {}", bin, pretool_flag())
    } else {
        format!("{} {} {}", bin, pretool_flag(), rest)
    }
}
