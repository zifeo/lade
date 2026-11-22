use std::path::Path;

use rusqlite::{Connection, params};
use serde_json::json;

use crate::event::Event;

use super::share::{Places, redact_for_share, share_path, share_text};
use super::source::{attach_read_only, is_tar_gz, query_attached};

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
    assert_eq!(out.command, "~/bin/deploy");
    assert_eq!(out.agent, Some(json!({"harness": "cursor"})));
}

#[test]
fn redact_cuts_a_prefix_only_at_a_directory_boundary() {
    let home_only = Places {
        repo: None,
        home: "/Users/sam",
    };
    assert_eq!(share_path(&home_only, "/Users/samantha/bin/x"), "x");
    assert_eq!(share_path(&home_only, "/Users/sam/bin/x"), "~/bin/x");
    assert_eq!(share_path(&home_only, "/Users/sam"), "~");
    let with_repo = Places {
        repo: Some("/srv/proj"),
        home: "/Users/sam",
    };
    assert_eq!(share_path(&with_repo, "/srv/projector/x"), "x");
    assert_eq!(share_path(&with_repo, "/srv/proj/x"), "x");
    assert_eq!(share_path(&with_repo, "/srv/proj/sub/x"), "sub/x");
    assert_eq!(share_path(&with_repo, "/srv/proj"), ".");
}

#[test]
fn a_pack_names_every_path_relative_to_the_repo_or_home() {
    let event = Event {
        id: "1".into(),
        ts: "2026-09-05T00:00:00.000Z".into(),
        kind: "access".into(),
        via: Some("pretool".into()),
        audience: Some("human".into()),
        actor: Some("alice".into()),
        repo: Some("/Users/alice/proj".into()),
        git_commit: Some("a".repeat(40)),
        command: "/Users/alice/proj/bin/deploy".into(),
        command_truncated: false,
        argv: Some(json!([
            "--config=/Users/alice/.aws/config",
            "/opt/tools/terraform",
            "-I/Users/alice/proj/include",
            "PATH=/usr/local/bin:/Users/alice/bin",
            "https://example.com/a/b",
            "rel/x",
        ])),
        hydrate_ms: Some(1.0),
        matches: json!([{
            "file": "/Users/alice/proj/infra",
            "rule": "^/Users/alice/bin/tf",
            "bindings": [
                {"key": "T", "uri": "file:///Users/alice/secrets.json?query=.t", "family": "secret"},
                {"key": "U", "uri": "op://vault/item/field", "family": "secret"},
            ],
        }]),
        agent: Some(json!({
            "harness": "cursor",
            "launch": "/Users/alice/.local/bin/engram --dir '/Users/alice/proj/data'",
        })),
    };
    let out = redact_for_share(&event, Path::new("/Users/alice")).unwrap();
    assert_eq!(out.command, "bin/deploy");
    assert_eq!(out.repo.as_deref(), Some("proj"));
    assert_eq!(
        out.argv,
        Some(json!([
            "--config=~/.aws/config",
            "terraform",
            "-Iinclude",
            "PATH=bin:~/bin",
            "https://example.com/a/b",
            "rel/x",
        ]))
    );
    assert_eq!(
        out.matches,
        json!([{
            "file": "infra",
            "rule": "^~/bin/tf",
            "bindings": [
                {"key": "T", "uri": "file://~/secrets.json?query=.t", "family": "secret"},
                {"key": "U", "uri": "op://vault/item/field", "family": "secret"},
            ],
        }])
    );
    assert_eq!(
        out.agent,
        Some(json!({"harness": "cursor", "launch": "~/.local/bin/engram --dir 'data'"}))
    );
    let packed = serde_json::to_string(&out).unwrap();
    assert!(!packed.contains("/Users"), "{packed}");
    assert!(!packed.contains("alice"), "{packed}");
}

#[test]
fn a_path_inside_a_word_stays_as_written() {
    let places = Places {
        repo: None,
        home: "/Users/sam",
    };
    assert_eq!(share_text(&places, "s/a/b/ x / y"), "s/a/b/ x / y");
    assert_eq!(share_text(&places, "cat /etc/hosts"), "cat hosts");
    assert_eq!(share_text(&places, "cc -L/opt/lib -lz"), "cc -Llib -lz");
    assert_eq!(share_text(&places, "lib/opt/x"), "lib/opt/x");
    assert_eq!(share_text(&places, "x/Users/sam/y"), "x~/y");
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

#[test]
fn a_source_is_attached_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a dir #1?").join("events.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE t (x INTEGER)")
        .unwrap();
    let conn = attach_read_only(std::slice::from_ref(&path)).unwrap();
    assert!(conn.is_readonly("p0").unwrap());
    conn.pragma_update(None, "query_only", false).unwrap();
    assert!(conn.execute("INSERT INTO p0.t VALUES (1)", []).is_err());
}
