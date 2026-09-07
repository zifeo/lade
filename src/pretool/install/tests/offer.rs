use super::super::agent::Agent;
use super::super::offer::{Plan, apply_plan};
use super::super::paths::short_path;
use super::super::skill::{SKILL_MD, is_lade_skill};
use super::super::ui::{default_scope, parse_agents, parse_scope, parse_yes_no};
use super::super::write::Scope;

#[test]
fn default_scope_is_repo_in_git_and_machine_otherwise() {
    assert_eq!(default_scope(true), Scope::Project);
    assert_eq!(default_scope(false), Scope::User);
}

#[test]
fn short_path_is_repo_relative_then_home() {
    let home = std::path::Path::new("/Users/me");
    let dest = std::path::Path::new("/Users/me/src/app");
    assert_eq!(
        short_path(&dest.join(".cursor").join("hooks.json"), home, dest),
        ".cursor/hooks.json"
    );
    assert_eq!(
        short_path(&home.join(".cursor").join("hooks.json"), home, dest),
        "~/.cursor/hooks.json"
    );
}

#[test]
fn parse_scope_empty_follows_the_stated_default() {
    assert_eq!(parse_scope("", Scope::Project).unwrap(), Scope::Project);
    assert_eq!(parse_scope("Y", Scope::Project).unwrap(), Scope::Project);
    assert_eq!(parse_scope("m", Scope::Project).unwrap(), Scope::User);
    assert_eq!(parse_scope("machine", Scope::Project).unwrap(), Scope::User);
    assert_eq!(parse_scope("local", Scope::User).unwrap(), Scope::Project);
    assert!(parse_scope("cloud", Scope::Project).is_err());
}

#[test]
fn parse_agents_accepts_slugs_and_rejects_unknown() {
    assert_eq!(
        parse_agents("cursor, claude").unwrap(),
        vec![Agent::Cursor, Agent::Claude]
    );
    assert_eq!(
        parse_agents("codex opencode").unwrap(),
        vec![Agent::Codex, Agent::OpenCode]
    );
    assert!(parse_agents("cursor cursor").unwrap() == vec![Agent::Cursor]);
    assert!(parse_agents("").is_err());
    assert!(parse_agents("vim").is_err());
}

#[test]
fn parse_yes_no_honors_the_stated_default() {
    assert!(parse_yes_no("", true).unwrap());
    assert!(!parse_yes_no("", false).unwrap());
    assert!(parse_yes_no("Y", false).unwrap());
    assert!(!parse_yes_no("n", true).unwrap());
    assert!(parse_yes_no("maybe", true).is_err());
}

#[test]
fn apply_plan_writes_user_hooks_and_skills() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    apply_plan(
        &Plan {
            scope: Scope::User,
            hooks: true,
            skills: true,
            agents: vec![Agent::Cursor],
        },
        home.path(),
        cwd.path(),
    )
    .unwrap();
    let hook = home.path().join(".cursor").join("hooks.json");
    let skill = Agent::Cursor.skill_path(home.path());
    let body = std::fs::read_to_string(&hook).unwrap();
    assert!(
        Agent::Cursor
            .hook_uses_command(&body, "lade hook --harness cursor")
            .unwrap()
    );
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), SKILL_MD);
    assert!(!cwd.path().join(".cursor").join("hooks.json").exists());
}

#[test]
fn apply_plan_can_write_project_skills_only() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    apply_plan(
        &Plan {
            scope: Scope::Project,
            hooks: false,
            skills: true,
            agents: vec![Agent::Claude],
        },
        home.path(),
        cwd.path(),
    )
    .unwrap();
    let skill = cwd
        .path()
        .join(".claude")
        .join("skills")
        .join("lade")
        .join("SKILL.md");
    assert!(is_lade_skill(&std::fs::read_to_string(&skill).unwrap()));
    assert!(!home.path().join(".claude").join("settings.json").exists());
    assert!(!cwd.path().join(".claude").join("settings.json").exists());
}
