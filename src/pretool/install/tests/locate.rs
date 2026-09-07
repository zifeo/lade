use super::super::agent::Agent;
use super::super::locate::{find_project, project_files};

#[test]
fn cursor_project_files_are_hooks_json() {
    let dir = std::path::Path::new("/tmp/proj");
    assert_eq!(
        project_files(Agent::Cursor, dir),
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
    let (path, installed) = find_project(Agent::Codex, home.path(), &repo).unwrap();
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
    let (path, installed) = find_project(Agent::Codex, home.path(), &nested).unwrap();
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
    let (path, installed) = find_project(Agent::Codex, home.path(), &repo).unwrap();
    assert!(installed);
    assert_eq!(path, repo.join(".codex").join("hooks.json"));
}
