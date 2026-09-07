/*
preToolUse handler for Cursor, Claude Code, and Codex (`lade hook`).

`detect()` classifies this process as Via::Pretool / Audience::Agent. Matching
commands are rewritten into `<lade> --pretool=<id> '…'` (the inject alias) so the
child keeps that classification via `--pretool=<id>`. Via lives on the ticket.
Disclaimer enforcement lives in inject.

# Cursor preToolUse — https://cursor.com/docs/agent/hooks (verified June 2026)
- Env: `CURSOR_VERSION`, `CURSOR_PROJECT_DIR`
- Input: `{"tool_name": "Shell", "tool_input": {"command": "..."}, "hook_event_name": "preToolUse", ...}`
- Output: `{"permission": "allow", "updated_input": {...}}`

# Claude-compatible PreToolUse (Claude Code, Codex)
- Claude: `CLAUDE_PROJECT_DIR` — https://code.claude.com/docs/en/hooks
- Codex: `CODEX_THREAD_ID` / `CODEX_SANDBOX` / `CODEX_HOME`, plus `turn_id`/`model`
  — https://developers.openai.com/codex/hooks
- OpenCode: `--harness opencode`, or `OPENCODE` / `OPENCODE_DIR` /
  `hook_source=opencode-plugin`
- Input (Claude-compat): `{"tool_name": "Bash"|"bash",
  "tool_input": {"command": "..."}, "hook_event_name": "PreToolUse", ...}`
- Output (Claude-compat): `{"hookSpecificOutput": {"hookEventName": "PreToolUse",
  "permissionDecision": "allow", "updatedInput": {...}}}`. Exit 0 with no
  stdout allows the original command. Shell tools match as `Bash`.
- OpenCode plugin input: `{"command": "...", "session_id": "..."}`.
  Output: `{"command": "..."}`.
*/

pub mod install;
mod platform;
mod response;
#[cfg(test)]
mod tests;
mod verb;

use crate::audience::Via;
use crate::config::{Audience, Config};
use crate::event::{self, Emit, Kind};
use crate::global_config::GlobalConfig;
use crate::message_box::MessageBox;
use crate::ticket::{PreEvent, TicketNetwork, write as write_ticket};
use anyhow::Result;
use serde_json::{Value, json};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use platform::{
    collect_agent, extract_command, has_pretool_stamp, is_already_injected, resolve_platform,
    split_env_prefix,
};
use verb::{hook_event, is_post_event, is_verb, normalize, verb_agent, verb_args};

pub(crate) use platform::split_env_prefix as split_command_env_prefix;
use response::{format_allow, format_allow_verb, format_modify};

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

pub fn handle(
    config: &Config,
    input: &str,
    audience: Audience,
    harness: Option<&str>,
) -> Result<String> {
    let parsed: Value = match serde_json::from_str(input) {
        Ok(value) => value,
        Err(_) => {
            warn_unread("hook stdin is not JSON");
            return Ok(allow(resolve_platform(harness, &json!({}))));
        }
    };
    let platform = resolve_platform(harness, &parsed);
    if is_post_event(&parsed) {
        return Ok(allow_verb(platform));
    }
    if is_verb(&parsed) {
        return handle_verb(config, &parsed, platform, audience);
    }
    let agent = collect_agent(platform, &parsed);

    let raw = match extract_command(&parsed) {
        Some(cmd) => cmd,
        None => {
            if expects_shell_command(&parsed) {
                warn_unread("hook payload has no command to wrap");
            }
            return Ok(allow(platform));
        }
    };

    // Keep any leading `LADE_APPROVE=...` (or other env assignments) so the
    // approval prefix reaches the wrapped process.
    let (env_prefix, command) = split_env_prefix(&raw);
    let tool_input = parsed.get("tool_input").cloned().unwrap_or(json!({}));

    if is_already_injected(&command) {
        if has_pretool_stamp(&env_prefix, &command) {
            return Ok(allow(platform));
        }
        let stamped = insert_pretool_flag(&command);
        let rewritten = if env_prefix.is_empty() {
            stamped
        } else {
            format!("{} {}", env_prefix, stamped)
        };
        return Ok(modify(platform, &tool_input, &rewritten));
    }

    let patterned = config.collect_for_with_pattern(&command, audience);
    if patterned.is_empty() {
        if config.log_on_walk() {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let saved_user = GlobalConfig::user_from_disk();
            event::emit_if(
                true,
                Emit {
                    kind: Kind::Seen,
                    via: Via::Pretool,
                    audience,
                    actor: event::actor(&saved_user),
                    cwd,
                    command: command.clone(),
                    argv: None,
                    hydrated: None,
                    matches: json!([]),
                    hydrate_ms: None,
                    agent: crate::agent_meta::merge(agent.clone()),
                },
            );
        }
        return Ok(allow(platform));
    }

    let saved_user = GlobalConfig::user_from_disk();
    let work = match Config::pre_event_work(&patterned, &saved_user) {
        Ok(work) => work,
        Err(_) => return Ok(allow(platform)),
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let pre = PreEvent {
        command: command.clone(),
        cwd: cwd.clone(),
        via: "pretool".to_string(),
        audience: match audience {
            Audience::Agent => "agent",
            Audience::Human => "human",
        }
        .to_string(),
        actor: event::actor(&saved_user),
        log: work.log,
        disclaimers: work.disclaimers,
        secrets: work.secrets,
        network: work
            .network
            .into_iter()
            .map(|binding| TicketNetwork {
                key: binding.key,
                uri: binding.uri,
            })
            .collect(),
        matches: work.matches,
        op_sa: work.op_sa,
        agent,
        network_pids: Vec::new(),
        pending: false,
    };
    let id = match write_ticket(&pre) {
        Ok(id) => id,
        Err(_) => return Ok(allow(platform)),
    };
    let lade_bin = invoked_lade_bin();
    let escaped = command.replace('\'', "'\\''");
    let wrapped = format!("{} --pretool={} '{}'", lade_bin, id, escaped);
    let new_command = if env_prefix.is_empty() {
        wrapped
    } else {
        format!("{} {}", env_prefix, wrapped)
    };
    Ok(modify(platform, &tool_input, &new_command))
}

fn handle_verb(
    config: &Config,
    parsed: &Value,
    platform: Option<platform::Platform>,
    audience: Audience,
) -> Result<String> {
    let command = normalize(parsed);
    let argv = verb_args(parsed);
    let patterned = config.collect_for_with_pattern(&command, audience);
    let saved_user = GlobalConfig::user_from_disk();
    let (log, matches) = if patterned.is_empty() {
        (config.log_on_walk(), json!([]))
    } else {
        match Config::pre_event_work(&patterned, &saved_user) {
            Ok(work) => (work.log, work.matches),
            Err(_) => (config.log_on_walk(), json!([])),
        }
    };
    let agent = crate::agent_meta::merge(verb_agent(collect_agent(platform, parsed), parsed));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    event::emit_verb(
        log,
        hook_event(parsed),
        parsed.get("tool_use_id").and_then(Value::as_str),
        Emit {
            kind: Kind::Seen,
            via: Via::Mcp,
            audience,
            actor: event::actor(&saved_user),
            cwd,
            command,
            argv,
            hydrated: None,
            matches,
            hydrate_ms: None,
            agent,
        },
    );
    Ok(allow_verb(platform))
}

fn expects_shell_command(input: &Value) -> bool {
    matches!(
        input.get("hook_event_name").and_then(Value::as_str),
        Some("PreToolUse" | "preToolUse")
    ) || input.get("tool_input").is_some()
        || input.get("hook_source").and_then(Value::as_str) == Some("opencode-plugin")
}

fn warn_unread(reason: &str) {
    MessageBox::new()
        .warning()
        .line(format!("Could not read this hook payload ({reason})."))
        .line("The command will run without injection.")
        .print_stderr();
}

fn allow(platform: Option<platform::Platform>) -> String {
    match platform {
        Some(platform) => format_allow(&platform),
        None => String::new(),
    }
}

fn allow_verb(platform: Option<platform::Platform>) -> String {
    match platform {
        Some(platform) => format_allow_verb(&platform),
        None => "{}".to_string(),
    }
}

fn modify(platform: Option<platform::Platform>, tool_input: &Value, new_command: &str) -> String {
    match platform {
        Some(platform) => format_modify(&platform, tool_input, new_command),
        None => format_modify(&platform::Platform::ClaudeCode, tool_input, new_command),
    }
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
