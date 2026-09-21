use super::super::agent::Agent;
use super::super::offer::{Plan, apply_plan, setup_at};
use super::super::paths::short_path;
use super::super::ui::{parse_harnesses, parse_yes_no};
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
fn parse_harnesses_accepts_slugs_and_rejects_unknown() {
    assert_eq!(
        parse_harnesses("cursor, claude").unwrap(),
        vec![Agent::Cursor, Agent::Claude]
    );
    assert_eq!(
        parse_harnesses("codex opencode").unwrap(),
        vec![Agent::Codex, Agent::OpenCode]
    );
    assert!(parse_harnesses("cursor cursor").unwrap() == vec![Agent::Cursor]);
    assert!(parse_harnesses("").is_err());
    assert!(parse_harnesses("vim").is_err());
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
fn setup_outside_git_writes_no_pretool_files() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    let report = setup_at(false, &[], home.path(), cwd.path()).unwrap();
    assert_eq!(report.where_line, "not a git repo, pre-tool skipped");
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
