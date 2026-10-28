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

pub(crate) mod bin;
mod classify;
mod cwd;
pub(crate) use cwd::{run_dir, same_dir};
pub mod install;
mod platform;
mod response;
#[cfg(test)]
mod tests;
mod verb;

use crate::audience::Via;
use crate::config::{Audience, Config, LadeFile};
use crate::event::{self, Emit, Kind};
use crate::global_config::GlobalConfig;
use crate::message_box::MessageBox;
use crate::ticket::{PreEvent, TicketNetwork, write as write_ticket};
use anyhow::Result;
use classify::{Class, classify, join_prefix, sanitize_env_prefix, wrap_idless, wrap_ticket};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use platform::{collect_agent, extract_command, resolve_platform, split_plain_env_prefix};
use verb::{hook_event, is_post_event, is_verb, normalize, verb_agent, verb_args};

pub(crate) use bin::invoked_lade_bin;
pub(crate) use platform::split_env_prefix as split_command_env_prefix;
use response::{allow, allow_verb, modify};

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
    // approval prefix reaches the wrapped process. Protocol leftovers are dropped.
    let (raw_prefix, rest) = split_plain_env_prefix(&raw);
    let prefix = sanitize_env_prefix(&raw_prefix);
    let tool_input = parsed.get("tool_input").cloned().unwrap_or(json!({}));
    let class = classify(&rest);
    match class {
        Class::Finished => return Ok(allow(platform)),
        Class::Empty => {
            emit_seen_pretool(
                config.log_on_walk_for(audience),
                audience,
                &rest,
                json!([]),
                agent,
                config.start(),
            );
            return Ok(allow(platform));
        }
        Class::Verb | Class::NotLade | Class::Vehicle { .. } => {}
    }
    let from_vehicle = matches!(class, Class::Vehicle { .. });
    let command = class.inner().unwrap_or(rest);

    let (dir, matched) = cwd::run_dir(&cwd::payload_dir(&parsed, config.start()), &command);
    let moved;
    let config = if cwd::same_dir(&dir, config.start()) {
        config
    } else {
        moved = match LadeFile::build(dir.clone()) {
            Ok(config) => config,
            Err(e) => {
                crate::config::report_load_error(&e);
                std::process::exit(crate::exit_codes::FAILURE);
            }
        };
        &moved
    };

    if raw_prefix.is_empty() && cwd::is_shell_move(&command) {
        emit_seen_pretool(
            config.log_on_walk_for(audience),
            audience,
            &command,
            json!([]),
            agent,
            &dir,
        );
        return Ok(done(platform, &tool_input, &prefix, &command, from_vehicle));
    }

    let patterned = config.collect_for_with_pattern(matched, audience);
    if patterned.is_empty() {
        emit_seen_pretool(
            config.log_on_walk_for(audience),
            audience,
            &command,
            json!([]),
            agent,
            &dir,
        );
        return Ok(done(platform, &tool_input, &prefix, &command, from_vehicle));
    }

    let saved_user = GlobalConfig::resolved_user();
    let work = match Config::pre_event_work(&patterned, &saved_user) {
        Ok(work) => work,
        Err(_) => {
            emit_seen_pretool(
                config.log_on_walk_for(audience),
                audience,
                &command,
                json!([]),
                agent,
                &dir,
            );
            return Ok(done(platform, &tool_input, &prefix, &command, from_vehicle));
        }
    };
    if !Config::needs_wrap(
        &work,
        matched,
        patterned.iter().map(|(_, _, rule)| rule),
        &saved_user,
    ) {
        emit_seen_pretool(work.log, audience, &command, work.matches, agent, &dir);
        return Ok(done(platform, &tool_input, &prefix, &command, from_vehicle));
    }
    let mut pre = PreEvent::new(
        &command,
        dir.clone(),
        Via::Pretool,
        audience,
        event::actor(&saved_user),
    );
    pre.log = work.log;
    pre.disclaimers = work.disclaimers;
    pre.secrets = work.secrets;
    pre.network = work
        .network
        .into_iter()
        .map(|binding| TicketNetwork {
            key: binding.key,
            uri: binding.uri,
            ttl: binding.ttl.map(|ttl| ttl.to_raw()),
        })
        .collect();
    pre.matches = work.matches.clone();
    pre.op_sa = work.op_sa;
    pre.agent = agent.clone();
    let lade_bin = invoked_lade_bin();
    let wrapped = match write_ticket(&pre) {
        Ok(id) => {
            crate::ticket::sweep_pretool(crate::ticket::PRETOOL_UNREAD);
            wrap_ticket(&lade_bin, &id, &command)
        }
        Err(_) => {
            warn_ticket();
            emit_seen_pretool(work.log, audience, &command, work.matches, agent, &dir);
            wrap_idless(&lade_bin, &command)
        }
    };
    Ok(modify(
        platform,
        &tool_input,
        &join_prefix(&prefix, &wrapped),
    ))
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
    let saved_user = GlobalConfig::resolved_user();
    let (log, matches) = if patterned.is_empty() {
        (config.log_on_walk_for(audience), json!([]))
    } else {
        match Config::pre_event_work(&patterned, &saved_user) {
            Ok(work) => (work.log, work.matches),
            Err(_) => (config.log_on_walk_for(audience), json!([])),
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

fn emit_seen_pretool(
    log: bool,
    audience: Audience,
    command: &str,
    matches: serde_json::Value,
    agent: serde_json::Value,
    cwd: &Path,
) {
    if !log {
        return;
    }
    let saved_user = GlobalConfig::resolved_user();
    event::emit_if(
        true,
        Emit {
            kind: Kind::Seen,
            via: Via::Pretool,
            audience,
            actor: event::actor(&saved_user),
            cwd: cwd.to_path_buf(),
            command: command.to_string(),
            argv: None,
            hydrated: None,
            matches,
            hydrate_ms: None,
            agent: crate::agent_meta::merge(agent),
        },
    );
}

fn done(
    platform: Option<platform::Platform>,
    tool_input: &Value,
    prefix: &str,
    inner: &str,
    rewrite_to_inner: bool,
) -> String {
    if rewrite_to_inner {
        modify(platform, tool_input, &join_prefix(prefix, inner))
    } else {
        allow(platform)
    }
}

fn warn_ticket() {
    MessageBox::new()
        .warning()
        .line("Could not write the pre-tool ticket.")
        .line("The command will run as inject without a ticket file.")
        .print_stderr();
}

fn warn_unread(reason: &str) {
    MessageBox::new()
        .warning()
        .line(format!("Could not read this hook payload ({reason})."))
        .line("The command will run without injection.")
        .print_stderr();
}
