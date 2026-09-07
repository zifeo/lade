use serde_json::Value;

use super::super::agent::Agent;
use super::super::offer::{Plan, apply_plan};
use super::super::paths::{ItemVerb, hook_command};
use super::super::write::{
    Scope, refresh_installed, refresh_path, uninstall_plane, uninstall_preferred, write_scoped,
};

#[test]
fn hook_command_never_writes_a_test_binary() {
    let command = hook_command(Agent::Codex);
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
        refresh_installed();
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), stale);
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
    refresh_path(Agent::Codex, &global, command);
    refresh_path(Agent::Claude, &other, command);
    let updated = std::fs::read_to_string(&global).unwrap();
    assert!(Agent::Codex.hook_uses_command(&updated, command).unwrap());
    assert!(!other.exists());
}

#[test]
fn bundled_hooks_match_repo_files() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(
        Agent::Cursor.snapshot(),
        std::fs::read_to_string(root.join(".cursor/hooks.json")).unwrap()
    );
    assert_eq!(
        Agent::Claude.snapshot(),
        std::fs::read_to_string(root.join(".claude/settings.json")).unwrap()
    );
    assert_eq!(
        Agent::Codex.snapshot(),
        std::fs::read_to_string(root.join(".codex/hooks.json")).unwrap()
    );
    assert_eq!(
        Agent::OpenCode.snapshot(),
        std::fs::read_to_string(root.join(".opencode/plugins/lade-pretool.js")).unwrap()
    );
}

#[test]
fn write_scoped_user_merges_claude_without_touching_other_keys() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let path = home.path().join(".claude").join("settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"permissions":{"allow":["Bash"]},"model":"sonnet","hooks":{}}"#,
    )
    .unwrap();
    let line = write_scoped(Scope::User, "claude", home.path(), cwd.path(), true).unwrap();
    assert!(line.contains("installed"));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["permissions"]["allow"][0], "Bash");
    assert_eq!(v["model"], "sonnet");
    assert!(
        Agent::Claude
            .has_hook(&std::fs::read_to_string(&path).unwrap())
            .unwrap()
    );
}

#[test]
fn uninstall_plane_removes_project_hook_and_skill() {
    let home = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    apply_plan(
        &Plan {
            scope: Scope::Project,
            hooks: true,
            skills: true,
            agents: vec![Agent::Cursor],
        },
        home.path(),
        dest.path(),
    )
    .unwrap();
    let report = uninstall_plane(Scope::Project, home.path(), dest.path()).unwrap();
    assert!(report.where_line.starts_with("this repo"));
    assert!(
        report
            .rows
            .iter()
            .any(|row| row.verb == ItemVerb::Removed && row.path == ".cursor/hooks.json")
    );
    assert!(report.rows.iter().any(|row| {
        row.verb == ItemVerb::Removed && row.path == ".cursor/skills/lade/SKILL.md"
    }));
    assert!(
        !Agent::Cursor
            .has_hook(
                &std::fs::read_to_string(dest.path().join(".cursor").join("hooks.json")).unwrap()
            )
            .unwrap()
    );
    assert!(
        !dest
            .path()
            .join(".cursor")
            .join("skills")
            .join("lade")
            .join("SKILL.md")
            .exists()
    );
}

#[test]
fn uninstall_preferred_falls_back_to_the_other_plane() {
    let home = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    apply_plan(
        &Plan {
            scope: Scope::User,
            hooks: true,
            skills: true,
            agents: vec![Agent::Codex],
        },
        home.path(),
        dest.path(),
    )
    .unwrap();
    let report = uninstall_preferred(Scope::Project, home.path(), dest.path()).unwrap();
    assert_eq!(report.where_line, "this machine");
    assert!(report.rows.iter().any(|row| row.verb == ItemVerb::Removed));
    assert!(
        !Agent::Codex
            .has_hook(&std::fs::read_to_string(Agent::Codex.config_path(home.path())).unwrap())
            .unwrap()
    );
}

#[test]
fn write_scoped_project_and_uninstall_round_trip() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    write_scoped(Scope::Project, "cursor", home.path(), cwd.path(), true).unwrap();
    let path = cwd.path().join(".cursor").join("hooks.json");
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("lade hook --harness cursor"));
    assert!(
        Agent::Cursor
            .hook_uses_command(&body, "lade hook --harness cursor")
            .unwrap()
    );
    write_scoped(Scope::Project, "cursor", home.path(), cwd.path(), false).unwrap();
    let cleaned = std::fs::read_to_string(&path).unwrap();
    assert!(!Agent::Cursor.has_hook(&cleaned).unwrap());
}

#[test]
fn codex_user_path_honors_codex_home() {
    let home = tempfile::tempdir().unwrap();
    let alt = tempfile::tempdir().unwrap();
    temp_env::with_var("CODEX_HOME", Some(alt.path().to_str().unwrap()), || {
        assert_eq!(
            Agent::Codex.config_path(home.path()),
            alt.path().join("hooks.json")
        );
    });
}
