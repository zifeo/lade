use super::config::{Agent, is_lade_hook};
use serde_json::Value;

const CMD: &str = "/usr/local/bin/lade hook";

#[test]
fn hook_command_never_writes_a_test_binary() {
    let command = super::hook_command(Agent::Codex);
    assert_eq!(command, "lade hook --harness codex");
}

#[test]
fn refresh_installed_is_inert_in_the_test_binary() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".codex").join("hooks.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let stale = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#;
    std::fs::write(&path, stale).unwrap();
    let home_str = home.path().to_str().unwrap();
    temp_env::with_var("HOME", Some(home_str), || {
        super::refresh_installed();
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), stale);
}

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
fn cursor_merge_from_empty_sets_version_and_hook() {
    let out = Agent::Cursor.merge("", CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["version"], 1);
    let arr = v["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(
        arr.iter()
            .any(|e| e["matcher"] == "Shell" && e["command"] == CMD)
    );
    assert!(
        arr.iter()
            .any(|e| e["matcher"] == "MCP:" && e["command"] == CMD)
    );
    assert_eq!(v["hooks"]["beforeMCPExecution"][0]["command"], CMD);
    assert!(v["hooks"]["beforeMCPExecution"][0].get("matcher").is_none());
    assert!(Agent::Cursor.has_hook(&out).unwrap());
}

#[test]
fn cursor_merge_is_idempotent() {
    let once = Agent::Cursor.merge("", CMD).unwrap();
    let twice = Agent::Cursor.merge(&once, "lade hook").unwrap();
    let v: Value = serde_json::from_str(&twice).unwrap();
    assert_eq!(v["hooks"]["preToolUse"].as_array().unwrap().len(), 2);
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
    assert_eq!(arr.len(), 3);
    assert!(arr.iter().any(|e| e["command"] == "other tool"));
    assert!(
        arr.iter()
            .any(|e| e["matcher"] == "Shell" && e["command"] == CMD)
    );
    assert!(
        arr.iter()
            .any(|e| e["matcher"] == "MCP:" && e["command"] == CMD)
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
fn claude_merge_from_empty_adds_bash_matcher() {
    let out = Agent::Claude.merge("", CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().any(|e| e["matcher"] == "Bash"));
    assert!(arr.iter().any(|e| e["matcher"] == "mcp__.*"));
    assert_eq!(arr[0]["hooks"][0]["type"], "command");
    assert!(arr.iter().all(|e| e["hooks"][0]["command"] == CMD));
    assert!(Agent::Claude.has_hook(&out).unwrap());
}

#[test]
fn claude_merge_is_idempotent() {
    let once = Agent::Claude.merge("", CMD).unwrap();
    let twice = Agent::Claude.merge(&once, "lade hook").unwrap();
    let v: Value = serde_json::from_str(&twice).unwrap();
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
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
    assert_eq!(arr.len(), 3);
    assert!(arr.iter().any(|e| e["matcher"] == "Write"));
    assert!(arr.iter().any(|e| e["matcher"] == "Bash"));
    assert!(arr.iter().any(|e| e["matcher"] == "mcp__.*"));
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
fn opencode_merge_writes_native_plugin() {
    let out = Agent::OpenCode.merge("", CMD).unwrap();
    assert!(out.contains("export const LadePretool"));
    assert!(out.contains("[\"hook\", \"--harness\", \"opencode\"]"));
    assert!(out.contains("session_id"));
    assert!(!out.contains("updatedInput"));
    assert!(!out.contains("OPENCODE:"));
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
fn codex_merge_from_empty_adds_bash_matcher() {
    let out = Agent::Codex.merge("", CMD).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().any(|e| e["matcher"] == "Bash"));
    assert!(arr.iter().any(|e| e["matcher"] == "mcp__.*"));
    assert_eq!(arr[0]["hooks"][0]["type"], "command");
    assert!(arr.iter().all(|e| e["hooks"][0]["command"] == CMD));
    assert!(Agent::Codex.has_hook(&out).unwrap());
}

#[test]
fn codex_merge_is_idempotent() {
    let once = Agent::Codex.merge("", CMD).unwrap();
    let twice = Agent::Codex.merge(&once, "lade hook").unwrap();
    let v: Value = serde_json::from_str(&twice).unwrap();
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
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
    assert_eq!(arr.len(), 3);
    assert!(arr.iter().any(|e| e["matcher"] == "apply_patch"));
    assert!(arr.iter().any(|e| e["matcher"] == "Bash"));
    assert!(arr.iter().any(|e| e["matcher"] == "mcp__.*"));
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
#[test]
fn cursor_project_merge_skips_before_mcp() {
    let out = Agent::Cursor.merge_scoped("", CMD, true).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert!(v["hooks"].get("beforeMCPExecution").is_none());
    assert!(
        Agent::Cursor
            .hook_uses_command_scoped(&out, CMD, true)
            .unwrap()
    );
    assert!(
        !Agent::Cursor
            .hook_uses_command_scoped(&out, CMD, false)
            .unwrap()
    );
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

#[test]
fn cursor_project_files_are_hooks_json() {
    let dir = std::path::Path::new("/tmp/proj");
    assert_eq!(
        super::project_files(Agent::Cursor, dir),
        vec![dir.join(".cursor").join("hooks.json")]
    );
}

#[test]
fn find_project_does_not_treat_home_codex_as_project() {
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("proj");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(
        home.path().join(".codex").join("hooks.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#,
    )
    .unwrap();
    let (path, installed) = super::find_project(Agent::Codex, home.path(), &repo).unwrap();
    assert!(!installed);
    assert_eq!(path, repo.join(".codex").join("hooks.json"));
}

#[test]
fn find_project_does_not_walk_to_root_when_cwd_is_outside_home() {
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(elsewhere.path().join(".codex")).unwrap();
    std::fs::write(
        elsewhere.path().join(".codex").join("hooks.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#,
    )
    .unwrap();
    let nested = elsewhere.path().join("proj");
    std::fs::create_dir_all(&nested).unwrap();
    let (path, installed) = super::find_project(Agent::Codex, home.path(), &nested).unwrap();
    assert!(!installed);
    assert_eq!(path, nested.join(".codex").join("hooks.json"));
}

#[test]
fn find_project_finds_repo_codex_hooks_before_home() {
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("proj");
    std::fs::create_dir_all(repo.join(".codex")).unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(
        home.path().join(".codex").join("hooks.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#,
    )
    .unwrap();
    std::fs::write(
        repo.join(".codex").join("hooks.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#,
    )
    .unwrap();
    let (path, installed) = super::find_project(Agent::Codex, home.path(), &repo).unwrap();
    assert!(installed);
    assert_eq!(path, repo.join(".codex").join("hooks.json"));
}

fn stale_official_skill() -> &'static str {
    super::STALE_OFFICIAL
}

#[test]
fn skill_path_sits_under_the_agent_home() {
    let home = std::path::Path::new("/tmp/home");
    assert_eq!(
        Agent::Cursor.skill_path(home),
        home.join(".cursor")
            .join("skills")
            .join("lade")
            .join("SKILL.md")
    );
    assert_eq!(
        Agent::OpenCode.skill_path(home),
        home.join(".config")
            .join("opencode")
            .join("skills")
            .join("lade")
            .join("SKILL.md")
    );
}

#[test]
fn bundled_skill_is_lade_managed() {
    assert!(super::is_lade_skill(super::SKILL_MD));
    assert!(super::skill_is_current(super::SKILL_MD));
    assert!(super::is_lade_skill(&stale_official_skill()));
    assert!(!super::skill_is_current(&stale_official_skill()));
    assert!(!super::is_lade_skill("# mine\n"));
}

#[test]
fn refresh_updates_stale_managed_skill_and_skips_unmanaged() {
    let home = tempfile::tempdir().unwrap();
    let path = Agent::Codex.skill_path(home.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, stale_official_skill()).unwrap();
    let foreign = Agent::Claude.skill_path(home.path());
    std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    std::fs::write(&foreign, "# mine\n").unwrap();
    super::refresh_at(home.path(), home.path());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), super::SKILL_MD);
    assert_eq!(std::fs::read_to_string(&foreign).unwrap(), "# mine\n");
}

#[test]
fn refresh_does_not_create_a_missing_skill() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    super::refresh_at(home.path(), home.path());
    assert!(!Agent::Codex.skill_path(home.path()).exists());
}

#[test]
fn refresh_updates_stale_hook_and_skips_missing() {
    let home = tempfile::tempdir().unwrap();
    let global = home.path().join(".codex").join("hooks.json");
    let other = home.path().join(".claude").join("settings.json");
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    let stale = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#;
    std::fs::write(&global, stale).unwrap();
    let command = "/usr/local/bin/lade hook --harness codex";
    super::refresh_path(Agent::Codex, &global, command, false);
    super::refresh_path(Agent::Claude, &other, command, false);
    let updated = std::fs::read_to_string(&global).unwrap();
    assert!(Agent::Codex.hook_uses_command(&updated, command).unwrap());
    assert!(!other.exists());
}
