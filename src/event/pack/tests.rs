use std::path::Path;

use rusqlite::{Connection, params};
use serde_json::json;

use crate::event::Event;

use super::share::{redact_for_share, sanitize_filename_component};
use super::source::{is_tar_gz, query_attached};

#[test]
fn sanitize_filename_component_replaces_unsafe_bytes() {
    assert_eq!(sanitize_filename_component("alice"), "alice");
    assert_eq!(sanitize_filename_component("a/b@c"), "a_b_c");
}

#[test]
fn is_tar_gz_matches_extension() {
    assert!(is_tar_gz(Path::new("pack.tar.gz")));
    assert!(!is_tar_gz(Path::new("pack.tgz")));
    assert!(!is_tar_gz(Path::new("pack.db")));
}

#[test]
fn redact_strips_home_and_git_root_prefixes() {
    let event = Event {
        id: "1".into(),
        ts: "2026-09-05T00:00:00.000Z".into(),
        kind: "seen".into(),
        via: None,
        audience: None,
        actor: Some("alice".into()),
        repo: Some("/Users/me/proj".into()),
        git_commit: None,
        command: "/Users/me/proj/bin/deploy".into(),
        command_truncated: false,
        argv: None,
        hydrate_ms: None,
        matches: json!([]),
        agent: Some(json!({"harness": "cursor"})),
    };
    let out = redact_for_share(&event, Path::new("/other")).unwrap();
    assert_eq!(out.command, "bin/deploy");
    assert_eq!(out.repo.as_deref(), Some("proj"));
    assert!(out.actor.is_none());
    assert_eq!(out.agent, Some(json!({"harness": "cursor"})));
    let home_first = Event {
        id: event.id,
        ts: event.ts,
        kind: event.kind,
        via: event.via,
        audience: event.audience,
        actor: Some("alice".into()),
        repo: Some("/Users/me/proj".into()),
        git_commit: event.git_commit,
        command: "/Users/me/bin/deploy".into(),
        command_truncated: false,
        argv: None,
        hydrate_ms: None,
        matches: json!([]),
        agent: event.agent.clone(),
    };
    let out = redact_for_share(&home_first, Path::new("/Users/me")).unwrap();
    assert_eq!(out.command, "bin/deploy");
    assert_eq!(out.agent, Some(json!({"harness": "cursor"})));
}

#[test]
fn share_keeps_empty_mcp_command() {
    let event = Event {
        id: "1".into(),
        ts: "2026-09-05T00:00:00.000Z".into(),
        kind: "seen".into(),
        via: Some("mcp".into()),
        audience: Some("agent".into()),
        actor: Some("alice".into()),
        repo: None,
        git_commit: None,
        command: String::new(),
        command_truncated: false,
        argv: None,
        hydrate_ms: None,
        matches: json!([]),
        agent: Some(json!({"launch": "engram", "hook": "beforeMCPExecution"})),
    };
    let out = redact_for_share(&event, Path::new("/Users/me")).unwrap();
    assert_eq!(out.command, "");
    assert_eq!(out.via.as_deref(), Some("mcp"));
    assert_eq!(out.agent.unwrap()["launch"], "engram");
}

#[test]
fn attached_query_reads_legacy_rows_without_agent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(crate::event::EVENTS_DDL).unwrap();
    conn.execute(
        "INSERT INTO events (
            id, ts, kind, via, audience, actor, repo, git_commit,
            command, command_truncated, hydrate_ms, matches
        ) VALUES (?1, ?2, 'seen', NULL, NULL, NULL, NULL, NULL, ?3, 0, NULL, jsonb(?4))",
        params!["1", "2026-09-05T00:00:00.000Z", "ls", "[]"],
    )
    .unwrap();
    drop(conn);
    let rows = query_attached(&[path], None, None, None, None, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].command, "ls");
    assert!(rows[0].agent.is_none());
}
