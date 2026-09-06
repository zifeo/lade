use chrono::{SecondsFormat, Utc};
use rusqlite::params;
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

fn emit_inner(e: Emit) -> rusqlite::Result<()> {
    let redacted = scrub::redact_command(&e.command, e.hydrated.as_ref());
    let (repo, git_commit) = git_stamp(&e.cwd);
    let event = Event {
        id: Uuid::now_v7().to_string(),
        ts: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        kind: e.kind.as_str().to_string(),
        via: Some(
            match e.via {
                Via::Preexec => "preexec",
                Via::Pretool => "pretool",
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
        command: redacted.command,
        command_truncated: redacted.truncated,
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
    tx.execute(
        "INSERT INTO events (
            id, ts, kind, via, audience, actor, repo, git_commit,
            command, command_truncated, hydrate_ms, matches, agent
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, jsonb(?12), jsonb(?13))",
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
        ],
    )?;
    tx.commit()
}
