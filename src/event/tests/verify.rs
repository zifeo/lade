use super::*;

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
fn verify_during_appends_never_reports_a_tampered_head() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit(seen_emit("first"));
        let writer = std::thread::spawn(|| {
            for i in 0..150 {
                emit(seen_emit(&format!("cmd{i}")));
            }
        });
        while !writer.is_finished() {
            let status = verify_live().unwrap();
            assert!(status.ok, "{status:?}");
        }
        writer.join().unwrap();
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

#[test]
fn opening_the_diary_never_scans_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit(seen_emit("echo"));
        let conn = open().unwrap();
        let plan: Vec<String> = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {}", chain::UNSEALED_PROBE))
            .unwrap()
            .query_map([], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            !plan.iter().any(|detail| detail.starts_with("SCAN events")),
            "{plan:?}"
        );
    });
}

#[test]
fn open_seals_rows_an_older_lade_left_unsealed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
        emit(seen_emit("echo"));
        emit(seen_emit("ls"));
        let conn = rusqlite::Connection::open(db_path()).unwrap();
        conn.execute(
            "UPDATE events SET seq = NULL, prev_hash = NULL, row_hash = NULL, seal_ver = NULL",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM chain_head", []).unwrap();
        emit(seen_emit("pwd"));
        let status = verify_live().unwrap();
        assert!(status.ok, "{status:?}");
        assert_eq!(status.rows, 3);
    });
}
