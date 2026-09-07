use serde_json::Value;

use super::super::agent::Agent;
use super::super::merge::is_lade_hook;
use super::CMD;

#[test]
fn is_lade_hook_matches_bare_and_absolute() {
    assert!(is_lade_hook("lade hook"));
    assert!(is_lade_hook("/usr/local/bin/lade hook"));
    assert!(is_lade_hook("target/debug/lade hook"));
    assert!(is_lade_hook("lade.exe hook"));
    assert!(is_lade_hook("lade hook --harness cursor"));
    assert!(is_lade_hook("/usr/local/bin/lade hook --harness claude"));
    assert!(!is_lade_hook("lade inject"));
    assert!(!is_lade_hook("blade hook"));
    assert!(!is_lade_hook("ladehook"));
    assert!(!is_lade_hook(""));
}

#[test]
fn cursor_merge_from_empty_is_repo_snapshot() {
    let out = Agent::Cursor
        .merge("", "lade hook --harness cursor")
        .unwrap();
    assert_eq!(out, Agent::Cursor.snapshot());
    let v: Value = serde_json::from_str(&out).unwrap();
    let arr = v["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["matcher"], "Shell");
    assert_eq!(arr[0]["command"], "lade hook --harness cursor");
    assert!(v["hooks"].get("beforeMCPExecution").is_none());
    assert!(Agent::Cursor.has_hook(&out).unwrap());
}

#[test]
fn cursor_merge_is_idempotent() {
    let once = Agent::Cursor.merge("", CMD).unwrap();
    let twice = Agent::Cursor.merge(&once, "lade hook").unwrap();
    let v: Value = serde_json::from_str(&twice).unwrap();
    assert_eq!(v["hooks"]["preToolUse"].as_array().unwrap().len(), 1);
    assert!(
        v["hooks"]["preToolUse"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["command"] == "lade hook")
    );
}

#[test]
fn hook_uses_command_distinguishes_bin() {
    let absolute = Agent::Cursor.merge("", CMD).unwrap();
    assert!(Agent::Cursor.hook_uses_command(&absolute, CMD).unwrap());
    assert!(
        !Agent::Cursor
            .hook_uses_command(&absolute, "lade hook")
            .unwrap()
    );
    let bare = Agent::Cursor.merge(&absolute, "lade hook").unwrap();
    assert!(Agent::Cursor.hook_uses_command(&bare, "lade hook").unwrap());
}

#[test]
fn cursor_merge_preserves_existing_hooks() {
    let existing = r#"{"version":1,"hooks":{"preToolUse":[{"command":"other tool","matcher":"Shell"}],"afterFileEdit":[{"command":"fmt"}]}}"#;
    let out = Agent::Cursor.merge(existing, CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    let arr = v["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().any(|e| e["command"] == "other tool"));
    assert!(
        arr.iter()
            .any(|e| e["matcher"] == "Shell" && e["command"] == CMD)
    );
    assert_eq!(v["hooks"]["afterFileEdit"][0]["command"], "fmt");
}

#[test]
fn cursor_remove_keeps_other_hooks() {
    let merged = Agent::Cursor
        .merge(
            r#"{"hooks":{"preToolUse":[{"command":"other","matcher":"Shell"}]}}"#,
            CMD,
        )
        .unwrap();
    let cleaned = Agent::Cursor.remove(&merged).unwrap();
    let v: Value = serde_json::from_str(&cleaned).unwrap();
    let arr = v["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["command"], "other");
    assert!(!Agent::Cursor.has_hook(&cleaned).unwrap());
}

#[test]
fn claude_merge_from_empty_is_repo_snapshot() {
    let out = Agent::Claude
        .merge("", "lade hook --harness claude")
        .unwrap();
    assert_eq!(out, Agent::Claude.snapshot());
    let v: Value = serde_json::from_str(&out).unwrap();
    let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["matcher"], "Bash");
    assert_eq!(arr[0]["hooks"][0]["command"], "lade hook --harness claude");
    assert!(Agent::Claude.has_hook(&out).unwrap());
}

#[test]
fn claude_merge_is_idempotent() {
    let once = Agent::Claude.merge("", CMD).unwrap();
    let twice = Agent::Claude.merge(&once, "lade hook").unwrap();
    let v: Value = serde_json::from_str(&twice).unwrap();
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert!(
        v["hooks"]["PreToolUse"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["hooks"][0]["command"] == "lade hook")
    );
}

#[test]
fn claude_merge_preserves_existing_settings() {
    let existing = r#"{"model":"sonnet","hooks":{"PreToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"guard"}]}]}}"#;
    let out = Agent::Claude.merge(existing, CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["model"], "sonnet");
    let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().any(|e| e["matcher"] == "Write"));
    assert!(arr.iter().any(|e| e["matcher"] == "Bash"));
}

#[test]
fn claude_remove_prunes_only_our_matcher() {
    let existing = r#"{"hooks":{"PreToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"guard"}]}]}}"#;
    let merged = Agent::Claude.merge(existing, CMD).unwrap();
    let cleaned = Agent::Claude.remove(&merged).unwrap();
    let v: Value = serde_json::from_str(&cleaned).unwrap();
    let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["matcher"], "Write");
    assert!(!Agent::Claude.has_hook(&cleaned).unwrap());
}

#[test]
fn has_hook_false_on_empty() {
    assert!(!Agent::Cursor.has_hook("").unwrap());
    assert!(!Agent::Claude.has_hook("   ").unwrap());
    assert!(!Agent::Codex.has_hook("").unwrap());
    assert!(!Agent::OpenCode.has_hook("").unwrap());
}

#[test]
fn opencode_merge_writes_repo_plugin() {
    let exact = Agent::OpenCode
        .merge("", "lade hook --harness opencode")
        .unwrap();
    assert_eq!(exact, Agent::OpenCode.snapshot());
    let out = Agent::OpenCode.merge("", CMD).unwrap();
    assert!(out.contains("export const LadePretool"));
    assert!(out.contains(r#"["hook", "--harness", "opencode"]"#));
    assert!(out.contains(CMD.split_whitespace().next().unwrap()));
    assert!(Agent::OpenCode.has_hook(&out).unwrap());
}

#[test]
fn opencode_config_path_is_plugin() {
    let home = std::path::Path::new("/tmp");
    let path = Agent::OpenCode.config_path(home);
    assert!(path.ends_with("plugins/lade-pretool.js"));
}

#[test]
fn codex_merge_from_empty_is_repo_snapshot() {
    let out = Agent::Codex.merge("", "lade hook --harness codex").unwrap();
    assert_eq!(out, Agent::Codex.snapshot());
    let v: Value = serde_json::from_str(&out).unwrap();
    let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["matcher"], "Bash");
    assert_eq!(arr[0]["hooks"][0]["command"], "lade hook --harness codex");
    assert!(Agent::Codex.has_hook(&out).unwrap());
}

#[test]
fn codex_merge_is_idempotent() {
    let once = Agent::Codex.merge("", CMD).unwrap();
    let twice = Agent::Codex.merge(&once, "lade hook").unwrap();
    let v: Value = serde_json::from_str(&twice).unwrap();
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert!(
        v["hooks"]["PreToolUse"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["hooks"][0]["command"] == "lade hook")
    );
}

#[test]
fn codex_merge_preserves_existing_hooks() {
    let existing = r#"{"description":"repo","hooks":{"PreToolUse":[{"matcher":"apply_patch","hooks":[{"type":"command","command":"guard"}]}]}}"#;
    let out = Agent::Codex.merge(existing, CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["description"], "repo");
    let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().any(|e| e["matcher"] == "apply_patch"));
    assert!(arr.iter().any(|e| e["matcher"] == "Bash"));
}

#[test]
fn codex_remove_prunes_only_our_matcher() {
    let existing = r#"{"hooks":{"PreToolUse":[{"matcher":"apply_patch","hooks":[{"type":"command","command":"guard"}]}]}}"#;
    let merged = Agent::Codex.merge(existing, CMD).unwrap();
    let cleaned = Agent::Codex.remove(&merged).unwrap();
    let v: Value = serde_json::from_str(&cleaned).unwrap();
    let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["matcher"], "apply_patch");
    assert!(!Agent::Codex.has_hook(&cleaned).unwrap());
}

#[test]
fn cursor_snapshot_has_no_mcp_matcher() {
    let out = Agent::Cursor.merge("", CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert!(v["hooks"].get("beforeMCPExecution").is_none());
    assert!(Agent::Cursor.hook_uses_command(&out, CMD).unwrap());
}

#[test]
fn merge_rejects_invalid_json() {
    assert!(Agent::Cursor.merge("{not json", CMD).is_err());
}

#[test]
fn merge_preserves_user_key_order() {
    let existing = r#"{"zebra":1,"alpha":2,"hooks":{"PreToolUse":[]}}"#;
    let out = Agent::Claude.merge(existing, CMD).unwrap();
    let zebra = out.find("zebra").expect("zebra kept");
    let alpha = out.find("alpha").expect("alpha kept");
    assert!(zebra < alpha, "user's original key order must be preserved");
}
