use chrono::{SecondsFormat, Utc};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::audience::Via;
use crate::config::Audience;
use crate::scrub;

use super::{Emit, Event, chain, git_stamp, open};

pub fn emit(e: Emit) {
    if let Err(err) = emit_inner(e) {
        log::debug!("lade event write failed: {err}");
    }
}

pub fn emit_if(log: bool, e: Emit) {
    if log {
        emit(e);
    }
}

pub fn emit_verb(log: bool, hook: Option<&str>, tool_use_id: Option<&str>, e: Emit) {
    if !log {
        return;
    }
    if let Err(err) = emit_verb_inner(hook, tool_use_id, e) {
        log::debug!("lade event write failed: {err}");
    }
}

fn emit_verb_inner(hook: Option<&str>, tool_use_id: Option<&str>, e: Emit) -> rusqlite::Result<()> {
    let Some(id) = tool_use_id.filter(|id| !id.is_empty()) else {
        emit_inner(e)?;
        return Ok(());
    };
    let mut conn = super::open()?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let existing: Option<(String, String, Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT id, command, json(agent), json(argv) FROM events
             WHERE json_extract(agent, '$.tool_use_id') = ?1
             ORDER BY ts DESC LIMIT 1",
            rusqlite::params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((event_id, command, raw, argv)) = existing else {
        chain::insert_sealed(&tx, &event_from_emit(e))?;
        return tx.commit();
    };
    match hook {
        Some("beforeMCPExecution") => {
            let mut event = event_by_id(&tx, &event_id)?;
            event.agent = Some(merge_launch(raw, e.agent.as_ref())).filter(|v| !v.is_null());
            write_updated(tx, &event_id, event)
        }
        Some("preToolUse") => {
            let fill_argv = json_missing(&argv);
            if command.is_empty() || fill_argv {
                let event = fill_verb_event(&tx, &event_id, raw, command.is_empty(), fill_argv, e)?;
                write_updated(tx, &event_id, event)
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn write_updated(
    tx: rusqlite::Transaction<'_>,
    event_id: &str,
    mut event: Event,
) -> rusqlite::Result<()> {
    if chain::is_head(&tx, event_id)? {
        chain::reseal_head(&tx, &event)?;
    } else {
        event.id = Uuid::now_v7().to_string();
        event.ts = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        chain::insert_sealed(&tx, &event)?;
    }
    tx.commit()
}

fn event_by_id(tx: &rusqlite::Transaction<'_>, id: &str) -> rusqlite::Result<Event> {
    tx.query_row(
        &format!("SELECT {} FROM events WHERE id = ?1", super::EVENT_COLS),
        rusqlite::params![id],
        super::event_from_row,
    )
}

fn json_missing(raw: &Option<String>) -> bool {
    match raw.as_deref() {
        None | Some("null") | Some("") => true,
        Some(value) => serde_json::from_str::<Value>(value)
            .ok()
            .is_none_or(|parsed| parsed.is_null()),
    }
}

fn merge_launch(raw: Option<String>, patch: Option<&Value>) -> Value {
    let mut agent: Value = raw
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(json!({}));
    if let Some(patch) = patch.and_then(Value::as_object)
        && let Some(obj) = agent.as_object_mut()
    {
        if let Some(launch) = patch.get("launch") {
            obj.insert("launch".to_string(), launch.clone());
        }
        if let Some(hook_name) = patch.get("hook") {
            obj.insert("hook".to_string(), hook_name.clone());
        }
    }
    agent
}

fn fill_verb_event(
    tx: &rusqlite::Transaction<'_>,
    event_id: &str,
    raw_agent: Option<String>,
    fill_command: bool,
    fill_argv: bool,
    e: Emit,
) -> rusqlite::Result<Event> {
    let mut event = event_by_id(tx, event_id)?;
    let redacted = scrub::redact_command(&e.command, None);
    let (command, truncated) = scrub::cap_text(redacted.command);
    let argv = scrub::redact_argv(e.argv, None);
    let mut agent: Value = raw_agent
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(json!({}));
    if let Some(patch) = e.agent.as_ref().and_then(Value::as_object)
        && let Some(obj) = agent.as_object_mut()
    {
        for (key, value) in patch {
            if key == "launch" && obj.contains_key("launch") {
                continue;
            }
            obj.insert(key.clone(), value.clone());
        }
    }
    event.agent = Some(agent).filter(|value| !value.is_null());
    event.matches = e.matches;
    if fill_command {
        event.command = command;
        event.command_truncated = truncated;
    }
    if fill_argv {
        event.argv = argv;
    }
    Ok(event)
}

fn event_from_emit(e: Emit) -> Event {
    let redacted = scrub::redact_command(&e.command, e.hydrated.as_ref());
    let (command, argv) = if e.argv.is_none() {
        scrub::peel_command(&redacted.command)
    } else {
        (redacted.command, e.argv)
    };
    let (command, truncated) = scrub::cap_text(command);
    let (repo, git_commit) = git_stamp(&e.cwd);
    Event {
        id: Uuid::now_v7().to_string(),
        ts: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        kind: e.kind.as_str().to_string(),
        via: Some(
            match e.via {
                Via::Preexec => "preexec",
                Via::Pretool => "pretool",
                Via::Mcp => Via::MCP,
                Via::Organic => "organic",
                Via::Unknown => "unknown",
            }
            .to_string(),
        ),
        audience: Some(
            match e.audience {
                Audience::Human => "human",
                Audience::Agent => "agent",
            }
            .to_string(),
        ),
        actor: e.actor,
        repo,
        git_commit,
        command,
        command_truncated: truncated,
        argv: scrub::redact_argv(argv, e.hydrated.as_ref()),
        hydrate_ms: e.hydrate_ms,
        matches: e.matches,
        agent: e.agent.filter(|value| !value.is_null()),
    }
}

fn emit_inner(e: Emit) -> rusqlite::Result<()> {
    let event = event_from_emit(e);
    let mut conn = open()?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    chain::insert_sealed(&tx, &event)?;
    tx.commit()
}
