use std::path::{Path, PathBuf};

use super::super::agent::Agent;
use super::super::locate::{canonical_project_path, find_project, project_dest};

fn git_dir(path: &Path) {
    std::fs::create_dir(path.join(".git")).unwrap();
}

fn write_codex_hook(dir: &Path) -> PathBuf {
    let path = dir.join(".codex").join("hooks.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#,
    )
    .unwrap();
    path
}

fn write_claude_hook(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook --harness claude"}]}]}}"#,
    )
    .unwrap();
}

#[test]
fn cursor_project_dest_is_hooks_json() {
    let dir = Path::new("/tmp/proj");
    assert_eq!(
        canonical_project_path(Agent::Cursor, dir),
        dir.join(".cursor").join("hooks.json")
    );
}

#[test]
fn claude_project_dest_is_settings_json() {
    let dir = Path::new("/tmp/proj");
    assert_eq!(
        canonical_project_path(Agent::Claude, dir),
        dir.join(".claude").join("settings.json")
    );
}

#[test]
fn find_project_does_not_treat_home_codex_as_project() {
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("proj");
    std::fs::create_dir_all(&repo).unwrap();
    write_codex_hook(home.path());
    let (path, installed) = find_project(Agent::Codex, home.path(), &repo).unwrap();
    assert!(!installed);
    assert_eq!(path, repo.join(".codex").join("hooks.json"));
}

#[test]
fn project_dest_skips_a_git_init_home() {
    let home = tempfile::tempdir().unwrap();
    git_dir(home.path());
    let nested = home.path().join("src");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(project_dest(home.path(), home.path()), None);
    assert_eq!(project_dest(home.path(), &nested), None);
}

#[test]
fn find_project_does_not_walk_when_cwd_is_outside_home() {
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    write_codex_hook(elsewhere.path());
    let nested = elsewhere.path().join("proj");
    std::fs::create_dir_all(&nested).unwrap();
    let (path, installed) = find_project(Agent::Codex, home.path(), &nested).unwrap();
    assert!(!installed);
    assert_eq!(path, nested.join(".codex").join("hooks.json"));
}

#[test]
fn find_project_uses_git_root_outside_home() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    git_dir(repo.path());
    let expected = write_codex_hook(repo.path());
    let nested = repo.path().join("src").join("app");
    std::fs::create_dir_all(&nested).unwrap();
    let dest = project_dest(home.path(), &nested).unwrap();
    let (path, installed) = find_project(Agent::Codex, home.path(), &nested).unwrap();
    assert!(installed);
    assert_eq!(path, dest.join(".codex").join("hooks.json"));
    assert_eq!(
        path.canonicalize().unwrap(),
        expected.canonicalize().unwrap()
    );
}

#[test]
fn find_project_uses_git_root_under_home() {
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("src").join("app");
    std::fs::create_dir_all(&repo).unwrap();
    git_dir(&repo);
    write_codex_hook(&repo);
    write_codex_hook(home.path());
    let nested = repo.join("crate");
    std::fs::create_dir_all(&nested).unwrap();
    let dest = project_dest(home.path(), &nested).unwrap();
    let (path, installed) = find_project(Agent::Codex, home.path(), &nested).unwrap();
    assert!(installed);
    assert_eq!(path, dest.join(".codex").join("hooks.json"));
}

#[test]
fn find_project_finds_repo_codex_hooks_before_home() {
    let home = tempfile::tempdir().unwrap();
    let repo = home.path().join("proj");
    std::fs::create_dir_all(&repo).unwrap();
    git_dir(&repo);
    write_codex_hook(home.path());
    write_codex_hook(&repo);
    let dest = project_dest(home.path(), &repo).unwrap();
    let (path, installed) = find_project(Agent::Codex, home.path(), &repo).unwrap();
    assert!(installed);
    assert_eq!(path, dest.join(".codex").join("hooks.json"));
}

#[test]
fn find_project_ignores_claude_settings_local() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    git_dir(repo.path());
    write_claude_hook(&repo.path().join(".claude").join("settings.local.json"));
    let dest = project_dest(home.path(), repo.path()).unwrap();
    let (path, installed) = find_project(Agent::Claude, home.path(), repo.path()).unwrap();
    assert!(!installed);
    assert_eq!(path, dest.join(".claude").join("settings.json"));
    assert!(
        repo.path()
            .join(".claude")
            .join("settings.local.json")
            .is_file()
    );
}
