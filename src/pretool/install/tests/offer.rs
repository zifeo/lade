use super::super::agent::Agent;
use super::super::offer::{Plan, apply_plan, setup_at};
use super::super::paths::{ItemVerb, short_path};
use super::super::skill::WHY_NO_SKILL;
use super::super::ui::{parse_agents, parse_yes_no};
use super::super::write::Scope;

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

fn git_dir(path: &std::path::Path) {
    std::fs::create_dir(path.join(".git")).unwrap();
}

#[test]
fn setup_outside_git_writes_no_agent_files() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    let report = setup_at(false, &[], home.path(), cwd.path()).unwrap();
    assert_eq!(report.where_line, "not a git repo, agents skipped");
    assert!(report.rows.is_empty());
    assert!(!home.path().join(".cursor").join("hooks.json").exists());
    assert!(!cwd.path().join(".cursor").join("hooks.json").exists());
}

#[test]
fn setup_in_git_writes_project_hooks_only() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    git_dir(cwd.path());
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    let report = setup_at(false, &["cursor"], home.path(), cwd.path()).unwrap();
    assert!(report.where_line.starts_with("this repo"));
    let hook = cwd.path().join(".cursor").join("hooks.json");
    let body = std::fs::read_to_string(&hook).unwrap();
    assert!(
        Agent::Cursor
            .hook_uses_command(&body, "lade hook --harness cursor")
            .unwrap()
    );
    assert!(!home.path().join(".cursor").join("hooks.json").exists());
    assert!(
        !cwd.path()
            .join(".cursor")
            .join("skills")
            .join("lade")
            .join("SKILL.md")
            .exists()
    );
}

#[test]
fn setup_flags_home_hooks_and_does_not_stack() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    git_dir(cwd.path());
    apply_plan(
        &Plan {
            scope: Scope::User,
            agents: vec![Agent::Cursor],
        },
        home.path(),
        cwd.path(),
    )
    .unwrap();
    let report = setup_at(false, &["cursor"], home.path(), cwd.path()).unwrap();
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].verb, super::super::paths::ItemVerb::Flagged);
    assert!(!cwd.path().join(".cursor").join("hooks.json").exists());
}

#[test]
fn setup_errors_when_both_planes_exist() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    git_dir(cwd.path());
    apply_plan(
        &Plan {
            scope: Scope::User,
            agents: vec![Agent::Cursor],
        },
        home.path(),
        cwd.path(),
    )
    .unwrap();
    apply_plan(
        &Plan {
            scope: Scope::Project,
            agents: vec![Agent::Cursor],
        },
        home.path(),
        cwd.path(),
    )
    .unwrap();
    let err = setup_at(false, &["cursor"], home.path(), cwd.path()).unwrap_err();
    assert!(
        err.to_string().contains("home and repo hooks both present"),
        "{err}"
    );
}

fn official_skill() -> &'static str {
    "---\nname: lade\ndescription: Use Lade safely.\n---\n\nLade is also called AD, AID, or LAID.\n"
}

fn write_skill(path: &std::path::Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

#[test]
fn setup_removes_lade_skills_at_home_and_in_the_repo() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    git_dir(cwd.path());
    let home_skill = home
        .path()
        .join(".cursor")
        .join("skills")
        .join("lade")
        .join("SKILL.md");
    let repo_skill = cwd
        .path()
        .join(".claude")
        .join("skills")
        .join("lade")
        .join("SKILL.md");
    let foreign = home
        .path()
        .join(".codex")
        .join("skills")
        .join("lade")
        .join("SKILL.md");
    write_skill(&home_skill, official_skill());
    write_skill(&repo_skill, official_skill());
    write_skill(&foreign, "# mine\n");
    let report = setup_at(false, &[], home.path(), cwd.path()).unwrap();
    assert!(!home_skill.exists());
    assert!(!repo_skill.exists());
    assert_eq!(std::fs::read_to_string(&foreign).unwrap(), "# mine\n");
    let removed: Vec<_> = report
        .rows
        .iter()
        .filter(|row| row.agent == "skill")
        .collect();
    assert_eq!(removed.len(), 2);
    assert!(removed.iter().all(|row| row.verb == ItemVerb::Removed));
    assert!(removed.iter().all(|row| row.note == WHY_NO_SKILL));
}

#[test]
fn setup_outside_git_removes_home_skill_not_cwd_skill() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let home_skill = home
        .path()
        .join(".cursor")
        .join("skills")
        .join("lade")
        .join("SKILL.md");
    let cwd_skill = cwd
        .path()
        .join(".cursor")
        .join("skills")
        .join("lade")
        .join("SKILL.md");
    write_skill(&home_skill, official_skill());
    write_skill(&cwd_skill, official_skill());
    let report = setup_at(false, &[], home.path(), cwd.path()).unwrap();
    assert_eq!(report.where_line, "not a git repo, agents skipped");
    assert!(!home_skill.exists());
    assert!(cwd_skill.exists());
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].note, WHY_NO_SKILL);
}

#[test]
fn apply_plan_writes_user_hooks() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    apply_plan(
        &Plan {
            scope: Scope::User,
            agents: vec![Agent::Cursor],
        },
        home.path(),
        cwd.path(),
    )
    .unwrap();
    let hook = home.path().join(".cursor").join("hooks.json");
    let body = std::fs::read_to_string(&hook).unwrap();
    assert!(
        Agent::Cursor
            .hook_uses_command(&body, "lade hook --harness cursor")
            .unwrap()
    );
    assert!(!cwd.path().join(".cursor").join("hooks.json").exists());
}
