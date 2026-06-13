use super::config::{Agent, is_lade_hook};
use serde_json::Value;

const CMD: &str = "/usr/local/bin/lade hook";

#[test]
fn is_lade_hook_matches_bare_and_absolute() {
    assert!(is_lade_hook("lade hook"));
    assert!(is_lade_hook("/usr/local/bin/lade hook"));
    assert!(is_lade_hook("target/debug/lade hook"));
    assert!(is_lade_hook("lade.exe hook"));
    assert!(!is_lade_hook("lade inject"));
    assert!(!is_lade_hook("blade hook"));
    assert!(!is_lade_hook("ladehook"));
    assert!(!is_lade_hook(""));
}

#[test]
fn cursor_merge_from_empty_sets_version_and_hook() {
    let out = Agent::Cursor.merge("", CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["version"], 1);
    let arr = v["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["command"], CMD);
    assert_eq!(arr[0]["matcher"], "Shell");
    assert!(Agent::Cursor.has_hook(&out).unwrap());
}

#[test]
fn cursor_merge_is_idempotent() {
    let once = Agent::Cursor.merge("", CMD).unwrap();
    let twice = Agent::Cursor.merge(&once, "lade hook").unwrap();
    let v: Value = serde_json::from_str(&twice).unwrap();
    assert_eq!(v["hooks"]["preToolUse"].as_array().unwrap().len(), 1);
}

#[test]
fn cursor_merge_preserves_existing_hooks() {
    let existing = r#"{"version":1,"hooks":{"preToolUse":[{"command":"other tool","matcher":"Shell"}],"afterFileEdit":[{"command":"fmt"}]}}"#;
    let out = Agent::Cursor.merge(existing, CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    let arr = v["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().any(|e| e["command"] == "other tool"));
    assert!(arr.iter().any(|e| e["command"] == CMD));
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
fn claude_merge_from_empty_adds_bash_matcher() {
    let out = Agent::Claude.merge("", CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["matcher"], "Bash");
    assert_eq!(arr[0]["hooks"][0]["type"], "command");
    assert_eq!(arr[0]["hooks"][0]["command"], CMD);
    assert!(Agent::Claude.has_hook(&out).unwrap());
}

#[test]
fn claude_merge_is_idempotent() {
    let once = Agent::Claude.merge("", CMD).unwrap();
    let twice = Agent::Claude.merge(&once, "lade hook").unwrap();
    let v: Value = serde_json::from_str(&twice).unwrap();
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
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
