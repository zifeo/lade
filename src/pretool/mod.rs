/*
preToolUse handler for Cursor, Claude Code, and Codex (`lade hook`).

`detect()` classifies this process as Via::Pretool / Audience::Agent. Matching
commands are rewritten into `lade --pretool=<id> '…'` (the inject alias), a bare
`lade` the harness shell resolves on its PATH, so the child keeps that
classification via `--pretool=<id>`. Via lives on the ticket.
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

use bin::REWRITE_BIN;
pub(crate) use bin::invoked_lade_bin;
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
            // § f7eb9ymg0.a@y4qra
            warn_unread("hook stdin is not JSON");
            return Ok(allow(resolve_platform(harness, &json!({}))));
        }
    };
    let platform = resolve_platform(harness, &parsed);
    if is_post_event(&parsed) {
        // § hk9h1sqyt.e@n4vqj
        return Ok(allow_verb(platform));
    }
    if is_verb(&parsed) {
        // § hk9h1sqyt.e@xq8hm
        return handle_verb(config, &parsed, platform, audience);
    }
    let agent = collect_agent(platform, &parsed);

    let raw = match extract_command(&parsed) {
        Some(cmd) => cmd,
        None => {
            // § f7eb9ymg0.a@wavv0
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
            // § kk9qxrvgt@f2p8s
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
    let lade_line = !matches!(class, Class::NotLade);
    let command = class.inner().unwrap_or(rest);

    // § g6sqmp8tb.d@y47gv
    let (dir, matched) = cwd::run_dir(&cwd::payload_dir(&parsed, config.start()), &command);
    let moved;
    let config = if cwd::same_dir(&dir, config.start()) {
        config
    } else {
        // § one y4zs3xgg4.c@p3ge6
        moved = match LadeFile::build(dir.clone()) {
            Ok(config) => config,
            Err(e) => {
                return Ok(refuse_unloaded(
                    platform,
                    &tool_input,
                    &prefix,
                    &command,
                    lade_line,
                    &e,
                ));
            }
        };
        &moved
    };
    // § end g6sqmp8tb.d

    if raw_prefix.is_empty() && cwd::is_shell_move(&command) {
        // § hk9h1sqyt.d@5zyzy
        // Wrapped, a move would move only Lade's child, so it runs as sent
        // even when its rules cannot be planned; only its row is lost then.
        match (
            config.collect_for_with_pattern(matched, audience),
            GlobalConfig::resolved_user(),
        ) {
            (Ok(patterned), Ok(saved_user)) => {
                let (log, matches) = unwrapped_vote(config, &patterned, &saved_user, audience);
                emit_seen_pretool(log, audience, &command, matches, agent, &dir);
            }
            (Err(error), _) | (_, Err(error)) => warn_unplanned(&error),
        }
        return Ok(done(platform, &tool_input, &prefix, &command, from_vehicle));
    }

    // A harness runs the line bare when the hook itself fails, so a plan
    // error goes to the wrap, which plans again, names it, and exits 1
    // without running the line.
    // § t7kcndtra.d@0djzc
    let wrap_to_fail = || {
        let wrapped = wrap_idless(REWRITE_BIN, &command);
        modify(platform, &tool_input, &join_prefix(&prefix, &wrapped))
    };
    let Ok(patterned) = config.collect_for_with_pattern(matched, audience) else {
        return Ok(wrap_to_fail());
    };
    if patterned.is_empty() {
        // § kk9qxrvgt@jk6gw
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

    // § hk9h1sqyt.a@c7xjp
    // Rules with no body entry prepare the same for every user, so the user
    // decides only their row: the line runs as an unmatched one does.
    let (saved_user, user_unread) = match GlobalConfig::resolved_user() {
        Ok(saved_user) => (saved_user, false),
        Err(_) if patterned.iter().all(|(_, _, rule)| rule.secrets.is_empty()) => (None, true),
        Err(_) => return Ok(wrap_to_fail()),
    };
    // § end hk9h1sqyt.a
    let Ok(work) = Config::pre_event_work(&patterned, &saved_user) else {
        return Ok(wrap_to_fail());
    };
    if Config::command_package(&patterned, &saved_user).is_some() {
        // Only the wrap names the refusal; run bare, the line would ignore it.
        // § one vwj3wvrzs.a@8qmvm, hk9h1sqyt.a@vhk85
        return Ok(wrap_to_fail());
    }
    if !Config::needs_wrap(
        &work,
        matched,
        patterned.iter().map(|(_, _, rule)| rule),
        &saved_user,
    ) {
        // § hk9h1sqyt.a@yc90y
        emit_seen_pretool(work.log, audience, &command, work.matches, agent, &dir);
        return Ok(done(platform, &tool_input, &prefix, &command, from_vehicle));
    }
    if user_unread {
        return Ok(wrap_to_fail());
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
    pre.agent = agent.clone();
    // § bvttr6qk1.c@m0ma0
    let wrapped = match write_ticket(&pre) {
        Ok(id) => {
            // § gy20cgb72.d@abc76
            crate::ticket::sweep_pretool(crate::ticket::PRETOOL_UNREAD);
            wrap_ticket(REWRITE_BIN, &id, &command)
        }
        Err(_) => {
            warn_ticket();
            wrap_idless(REWRITE_BIN, &command)
        }
    };
    Ok(modify(
        platform,
        &tool_input,
        &join_prefix(&prefix, &wrapped),
    ))
    // § end bvttr6qk1.c
}

/// The hook's own directory has a walk that does not load.
pub fn handle_unloaded(error: &anyhow::Error, input: &str, harness: Option<&str>) -> String {
    let Ok(parsed) = serde_json::from_str::<Value>(input) else {
        // § f7eb9ymg0.a@3hfj4
        warn_unread("hook stdin is not JSON");
        return allow(resolve_platform(harness, &json!({})));
    };
    let platform = resolve_platform(harness, &parsed);
    if is_post_event(&parsed) {
        // § hk9h1sqyt.e@6kw3h
        return allow_verb(platform);
    }
    if is_verb(&parsed) {
        // § hk9h1sqyt.e@xna9t
        warn_unplanned(error);
        return allow_verb(platform);
    }
    let Some(raw) = extract_command(&parsed) else {
        // § f7eb9ymg0.a@revv3
        if expects_shell_command(&parsed) {
            warn_unread("hook payload has no command to wrap");
        }
        return allow(platform);
    };
    let (raw_prefix, rest) = split_plain_env_prefix(&raw);
    let tool_input = parsed.get("tool_input").cloned().unwrap_or(json!({}));
    let lade_line = !matches!(classify(&rest), Class::NotLade);
    refuse_unloaded(
        platform,
        &tool_input,
        &sanitize_env_prefix(&raw_prefix),
        &rest,
        lade_line,
        error,
    )
}

/// A line that starts Lade loads the walk itself and says why it stops.
fn refuse_unloaded(
    platform: Option<platform::Platform>,
    tool_input: &Value,
    prefix: &str,
    command: &str,
    lade_line: bool,
    error: &anyhow::Error,
) -> String {
    if lade_line {
        // § y4zs3xgg4.e@kfhrn
        return allow(platform);
    }
    // § y4zs3xgg4.d@hzwks
    let error = format!("{error:#}");
    // § one y4zs3xgg4.f@b4fbe
    let reason = format!(
        "Could not load a lade.yaml, so Lade refused this command: {}. Fix the file, then run the command again.",
        error.trim_end_matches('.')
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");
    // The OpenCode rewrite is the wrap, which names the error itself.
    if platform != Some(platform::Platform::OpenCode) {
        MessageBox::new()
            .error()
            .line(reason.clone())
            .print_stderr();
    }
    let failing = join_prefix(prefix, &wrap_idless(REWRITE_BIN, command));
    response::deny(platform, tool_input, &failing, &reason)
}

fn handle_verb(
    config: &Config,
    parsed: &Value,
    platform: Option<platform::Platform>,
    audience: Audience,
) -> Result<String> {
    let command = normalize(parsed);
    let argv = verb_args(parsed);
    // § hk9h1sqyt.e@7chme
    let (patterned, saved_user) = match (
        config.collect_for_with_pattern(&command, audience),
        GlobalConfig::resolved_user(),
    ) {
        (Ok(patterned), Ok(saved_user)) => (patterned, saved_user),
        (Err(error), _) | (_, Err(error)) => {
            warn_unplanned(&error);
            return Ok(allow_verb(platform));
        }
    };
    // § end hk9h1sqyt.e
    let (log, matches) = unwrapped_vote(config, &patterned, &saved_user, audience);
    let agent = crate::agent_meta::merge(verb_agent(collect_agent(platform, parsed), parsed));
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // § bvttr6qk1.d@2ryqm
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
    // § end bvttr6qk1.d
    Ok(allow_verb(platform))
}

/// `log` and `matches` of a line the hook records without a wrap.
fn unwrapped_vote(
    config: &Config,
    patterned: &[(PathBuf, String, crate::config::LadeRule)],
    saved_user: &Option<String>,
    audience: Audience,
) -> (bool, Value) {
    // § kk9qxrvgt@dn9wv
    if patterned.is_empty() {
        return (config.log_on_walk_for(audience), json!([]));
    }
    match Config::pre_event_work(patterned, saved_user) {
        Ok(work) => (work.log, work.matches),
        Err(_) => {
            let plain: Vec<_> = patterned
                .iter()
                .map(|(path, _, rule)| (path.clone(), rule.clone()))
                .collect();
            (Config::log_enabled(&plain), json!([]))
        }
    }
}

fn expects_shell_command(input: &Value) -> bool {
    matches!(
        input.get("hook_event_name").and_then(Value::as_str),
        Some("PreToolUse" | "preToolUse")
    ) || input.get("tool_input").is_some()
        || input.get("hook_source").and_then(Value::as_str) == Some("opencode-plugin")
}

/// A line it records runs as sent even when the user its row names cannot
/// be read, as a `cd` and an MCP verb do: only the row is lost then.
fn emit_seen_pretool(
    log: bool,
    audience: Audience,
    command: &str,
    matches: serde_json::Value,
    agent: serde_json::Value,
    cwd: &Path,
) {
    // § bvttr6qk1.a@tx1h9
    if !log {
        return;
    }
    // § hk9h1sqyt.a@e1xdx
    let saved_user = match GlobalConfig::resolved_user() {
        Ok(saved_user) => saved_user,
        Err(error) => {
            warn_unplanned(&error);
            return;
        }
    };
    // § end hk9h1sqyt.a
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

/// A line the hook allows without a wrap, whose rules it could not plan:
/// it runs, and no diary row records it.
fn warn_unplanned(error: &anyhow::Error) {
    MessageBox::new()
        .warning()
        .line(format!("Could not plan this line's rules ({error:#})."))
        .line("It runs as sent, with no diary row.")
        .print_stderr();
}

fn warn_unread(reason: &str) {
    MessageBox::new()
        .warning()
        .line(format!("Could not read this hook payload ({reason})."))
        .line("The command will run without injection.")
        .print_stderr();
}
