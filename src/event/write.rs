use chrono::{SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::audience::Via;
use crate::config::Audience;
use crate::scrub;

use super::{Emit, Event, git_stamp, open};

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
    match existing {
        Some((event_id, _, raw, _)) if hook == Some("beforeMCPExecution") => {
            let agent = merge_launch(raw, e.agent.as_ref());
            let encoded = serde_json::to_string(&agent).unwrap_or_else(|_| "null".into());
            tx.execute(
                "UPDATE events SET agent = jsonb(?2) WHERE id = ?1",
                rusqlite::params![event_id, encoded],
            )?;
            tx.commit()?;
        }
        Some((event_id, command, raw, argv)) if hook == Some("preToolUse") => {
            if command.is_empty() || json_missing(&argv) {
                fill_verb_row(
                    &tx,
                    &event_id,
                    raw,
                    command.is_empty(),
                    json_missing(&argv),
                    e,
                )?;
                tx.commit()?;
            }
        }
        Some(_) => {}
        None => {
            drop(tx);
            emit_inner(e)?;
        }
    }
    Ok(())
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

fn fill_verb_row(
    tx: &rusqlite::Transaction<'_>,
    event_id: &str,
    raw_agent: Option<String>,
    fill_command: bool,
    fill_argv: bool,
    e: Emit,
) -> rusqlite::Result<()> {
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
    let encoded_agent = serde_json::to_string(&agent).unwrap_or_else(|_| "null".into());
    let encoded_argv = argv
        .as_ref()
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "null".into()));
    let matches = serde_json::to_string(&e.matches).unwrap_or_else(|_| "[]".into());
    if fill_command && fill_argv {
        tx.execute(
            "UPDATE events SET command = ?2, command_truncated = ?3, argv = jsonb(?4),
                matches = jsonb(?5), agent = jsonb(?6)
             WHERE id = ?1",
            rusqlite::params![
                event_id,
                command,
                truncated as i64,
                encoded_argv,
                matches,
                encoded_agent
            ],
        )?;
    } else if fill_command {
        tx.execute(
            "UPDATE events SET command = ?2, command_truncated = ?3, matches = jsonb(?4),
                agent = jsonb(?5)
             WHERE id = ?1",
            rusqlite::params![event_id, command, truncated as i64, matches, encoded_agent],
        )?;
    } else {
        tx.execute(
            "UPDATE events SET argv = jsonb(?2), matches = jsonb(?3), agent = jsonb(?4)
             WHERE id = ?1",
            rusqlite::params![event_id, encoded_argv, matches, encoded_agent],
        )?;
    }
    Ok(())
}

fn emit_inner(e: Emit) -> rusqlite::Result<()> {
    let redacted = scrub::redact_command(&e.command, e.hydrated.as_ref());
    let (command, argv) = if e.argv.is_none() {
        scrub::peel_command(&redacted.command)
    } else {
        (redacted.command, e.argv)
    };
    let (command, truncated) = scrub::cap_text(command);
    let (repo, git_commit) = git_stamp(&e.cwd);
    let event = Event {
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
    };
    let mut conn = open()?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let matches = serde_json::to_string(&event.matches).unwrap_or_else(|_| "[]".into());
    let agent = event
        .agent
        .as_ref()
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "null".into()));
    let argv = event
        .argv
        .as_ref()
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "null".into()));
    tx.execute(
        "INSERT INTO events (
            id, ts, kind, via, audience, actor, repo, git_commit,
            command, command_truncated, hydrate_ms, matches, agent, argv
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, jsonb(?12), jsonb(?13), jsonb(?14))",
        params![
            event.id,
            event.ts,
            event.kind,
            event.via,
            event.audience,
            event.actor,
            event.repo,
            event.git_commit,
            event.command,
            event.command_truncated as i64,
            event.hydrate_ms,
            matches,
            agent,
            argv,
        ],
    )?;
    tx.commit()
}
