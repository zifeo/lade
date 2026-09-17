use super::*;
use serde_json::json;

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
