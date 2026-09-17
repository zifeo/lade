use super::*;
use crate::audience::Via;
use crate::config::Audience;
use serde_json::{Value, json};
use std::path::PathBuf;

#[test]
fn db_path_override_and_project_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let override_path = dir.path().join("events.db");
    temp_env::with_var(
        "LADE_EVENTS_PATH",
        Some(override_path.to_str().unwrap()),
        || {
            assert_eq!(db_path(), override_path);
        },
    );
    temp_env::with_var("LADE_EVENTS_PATH", None::<&str>, || {
        let expected = directories::ProjectDirs::from("com", "zifeo", "lade")
            .expect("cannot get directory for projet")
            .data_local_dir()
            .join("events.db");
        assert_eq!(db_path(), expected);
    });
}

#[test]
fn git_stamp_treats_gitfile_as_worktree_root() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = dir.path().join("wt");
    let gitdir = dir.path().join("main.git");
    std::fs::create_dir(&worktree).unwrap();
    std::fs::create_dir(&gitdir).unwrap();
    std::fs::write(
        gitdir.join("HEAD"),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", gitdir.display()),
    )
    .unwrap();
    let (repo, commit) = git_stamp(&worktree);
    assert_eq!(
        repo.as_deref(),
        Some(worktree.canonicalize().unwrap().to_str().unwrap())
    );
    assert_eq!(
        commit.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
}

fn verb_emit(command: &str, argv: Option<Value>, agent: Value) -> Emit {
    Emit {
        kind: Kind::Seen,
        via: Via::Mcp,
        audience: Audience::Agent,
        actor: None,
        cwd: PathBuf::from("."),
        command: command.into(),
        argv,
        hydrated: None,
        matches: json!([]),
        hydrate_ms: None,
        agent: Some(agent),
    }
}

#[test]
fn before_mcp_then_pretool_is_one_filled_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit_verb(
            true,
            Some("beforeMCPExecution"),
            Some("toolu_1"),
            verb_emit(
                "engram.mem_stats",
                None,
                json!({
                    "tool_use_id": "toolu_1",
                    "hook": "beforeMCPExecution",
                    "launch": "engram"
                }),
            ),
        );
        emit_verb(
            true,
            Some("preToolUse"),
            Some("toolu_1"),
            verb_emit(
                "engram.mem_stats",
                Some(json!({"project": "lade"})),
                json!({
                    "tool_use_id": "toolu_1",
                    "hook": "preToolUse",
                    "tool": "mem_stats"
                }),
            ),
        );
        let rows = query(None, None, None, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].command, "engram.mem_stats");
        assert_eq!(rows[0].argv, Some(json!({"project": "lade"})));
        let agent = rows[0].agent.as_ref().unwrap();
        assert_eq!(agent["launch"], "engram");
        assert_eq!(agent["hook"], "preToolUse");
        assert_eq!(agent["tool"], "mem_stats");
    });
}

#[test]
fn pretool_then_before_mcp_patches_launch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit_verb(
            true,
            Some("preToolUse"),
            Some("toolu_2"),
            verb_emit(
                "engram.mem_stats",
                Some(json!({"project": "lade"})),
                json!({"tool_use_id": "toolu_2", "hook": "preToolUse"}),
            ),
        );
        emit_verb(
            true,
            Some("beforeMCPExecution"),
            Some("toolu_2"),
            verb_emit(
                "engram.mem_stats",
                None,
                json!({
                    "tool_use_id": "toolu_2",
                    "hook": "beforeMCPExecution",
                    "launch": "engram"
                }),
            ),
        );
        let rows = query(None, None, None, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].argv, Some(json!({"project": "lade"})));
        assert_eq!(rows[0].agent.as_ref().unwrap()["launch"], "engram");
    });
}

#[test]
fn launch_only_before_mcp_writes_a_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit_verb(
            true,
            Some("beforeMCPExecution"),
            Some("toolu_3"),
            verb_emit(
                "",
                None,
                json!({
                    "tool_use_id": "toolu_3",
                    "hook": "beforeMCPExecution",
                    "launch": "engram"
                }),
            ),
        );
        let rows = query(None, None, None, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].command, "");
        assert_eq!(rows[0].via.as_deref(), Some("mcp"));
        assert_eq!(rows[0].agent.as_ref().unwrap()["launch"], "engram");
    });
}

fn seen_emit(command: &str) -> Emit {
    Emit {
        kind: Kind::Seen,
        via: Via::Organic,
        audience: Audience::Human,
        actor: None,
        cwd: PathBuf::from("."),
        command: command.into(),
        argv: None,
        hydrated: None,
        matches: json!([]),
        hydrate_ms: None,
        agent: None,
    }
}

#[test]
fn concurrent_first_open_keeps_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        let writers: Vec<_> = (0..8)
            .map(|i| {
                std::thread::spawn(move || {
                    emit(seen_emit(&format!("cmd{i}")));
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        let rows = query(None, None, None, None, None, None).unwrap();
        assert_eq!(rows.len(), 8);
        let status = verify_live().unwrap();
        assert!(status.ok, "{status:?}");
        assert_eq!(status.rows, 8);
    });
}

#[test]
fn sqlite_edit_breaks_verify() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit(seen_emit("echo"));
        emit(seen_emit("ls"));
        assert!(verify_live().unwrap().ok);
        let conn = rusqlite::Connection::open(db_path()).unwrap();
        conn.execute(
            "UPDATE events SET command = 'rm' WHERE command = 'echo'",
            [],
        )
        .unwrap();
        let status = verify_live().unwrap();
        assert!(!status.ok, "{status:?}");
        assert_eq!(status.breaks.len(), 1);
        assert_eq!(status.breaks[0].command.as_deref(), Some("rm"));
        assert_eq!(status.breaks[0].before, 0);
        assert_eq!(status.breaks[0].seq, Some(1));
        let line = status.report_lines("events.db").join("\n");
        assert!(line.contains("0 rows before it"), "{line}");
        assert!(line.contains("1 row after it"), "{line}");
    });
}

#[test]
fn later_edit_keeps_earlier_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit(seen_emit("echo"));
        emit(seen_emit("ls"));
        emit(seen_emit("pwd"));
        let conn = rusqlite::Connection::open(db_path()).unwrap();
        conn.execute("UPDATE events SET command = 'rm' WHERE command = 'ls'", [])
            .unwrap();
        let status = verify_live().unwrap();
        assert_eq!(status.breaks.len(), 1);
        assert_eq!(status.breaks[0].before, 1);
        assert_eq!(status.breaks[0].seq, Some(2));
        let line = status.report_lines("events.db").join("\n");
        assert!(line.contains("1 row before it"), "{line}");
        assert!(line.contains("1 row after it"), "{line}");
    });
}

#[test]
fn two_edits_are_both_reported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit(seen_emit("echo"));
        emit(seen_emit("ls"));
        let conn = rusqlite::Connection::open(db_path()).unwrap();
        conn.execute("UPDATE events SET command = 'rm'", [])
            .unwrap();
        let status = verify_live().unwrap();
        assert_eq!(status.breaks.len(), 2, "{status:?}");
        assert_eq!(status.breaks[0].before, 0);
        assert_eq!(status.breaks[1].before, 0);
    });
}

#[test]
fn seq_gap_marks_the_row_after_the_hole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit(seen_emit("echo"));
        emit(seen_emit("ls"));
        emit(seen_emit("pwd"));
        let conn = rusqlite::Connection::open(db_path()).unwrap();
        conn.execute("DROP TRIGGER events_no_delete", []).unwrap();
        conn.execute("DELETE FROM events WHERE command = 'ls'", [])
            .unwrap();
        let status = verify_live().unwrap();
        assert!(!status.ok, "{status:?}");
        assert!(
            status
                .breaks
                .iter()
                .any(|b| b.reason.contains("seq gap") || b.reason.contains("prev_hash")),
            "{status:?}"
        );
        assert_eq!(status.rows, 2);
    });
}

#[test]
fn delete_trigger_blocks_raw_sql() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit(seen_emit("echo"));
        let conn = rusqlite::Connection::open(db_path()).unwrap();
        let err = conn
            .execute("DELETE FROM events WHERE command = 'echo'", [])
            .unwrap_err();
        assert!(err.to_string().contains("append-only"), "{err}");
        assert!(verify_live().unwrap().ok);
    });
}
