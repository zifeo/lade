use std::fs;
use std::path::{Path, PathBuf};

use super::super::agent::{AGENTS, Agent};
use super::super::health::report_at;
use super::super::opencode::{OpenCodeApi, plugin};
use super::super::write::refresh_at;

/// A repo under a fresh home, with this OpenCode plugin.
fn repo_with_plugin(home: &Path, content: &str) -> (PathBuf, PathBuf) {
    let repo = home.join("src").join("app");
    let path = repo
        .join(".opencode")
        .join("plugins")
        .join("lade-pretool.js");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    (repo, path)
}

#[test]
fn current_hooks_need_nothing() {
    let home = tempfile::tempdir().unwrap();
    let (repo, _) = repo_with_plugin(home.path(), OpenCodeApi::V2.snapshot());
    assert!(report_at(home.path(), &repo, &AGENTS, &|| Some(OpenCodeApi::V2)).is_empty());
    assert!(report_at(home.path(), &repo, &AGENTS, &|| None).is_empty());
}

#[test]
fn plugin_api_must_match_the_installed_opencode() {
    let home = tempfile::tempdir().unwrap();
    let (repo, _) = repo_with_plugin(home.path(), OpenCodeApi::V1.snapshot());
    let lines = report_at(home.path(), &repo, &AGENTS, &|| Some(OpenCodeApi::V2));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("OpenCode v1 API, `opencode` is v2"),
        "{lines:?}"
    );
    assert!(
        lines[0].contains("`lade hook enable --harness opencode` rewrites it."),
        "{lines:?}"
    );
    assert!(report_at(home.path(), &repo, &AGENTS, &|| None).is_empty());
}

#[test]
fn refresh_keeps_the_api_and_unpins_a_repo_plugin() {
    let home = tempfile::tempdir().unwrap();
    let pinned = plugin(
        OpenCodeApi::V1,
        "/opt/lade/bin/lade hook --harness opencode",
    );
    let (repo, path) = repo_with_plugin(home.path(), &pinned);
    refresh_at(home.path(), &repo, &AGENTS);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        OpenCodeApi::V1.snapshot()
    );
}

#[test]
fn refresh_leaves_a_hand_edited_plugin_and_reports_it() {
    let home = tempfile::tempdir().unwrap();
    let edited = format!("{}\n// local tweak\n", OpenCodeApi::V2.snapshot());
    let (repo, path) = repo_with_plugin(home.path(), &edited);
    refresh_at(home.path(), &repo, &AGENTS);
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
    let lines = report_at(home.path(), &repo, &AGENTS, &|| Some(OpenCodeApi::V2));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("differs from what this Lade writes"),
        "{lines:?}"
    );
}

#[test]
fn invalid_json_is_reported_without_a_rewrite() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".cursor").join("hooks.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{not json").unwrap();
    let lines = report_at(
        home.path(),
        home.path(),
        &AGENTS,
        &|| -> Option<OpenCodeApi> {
            panic!("no OpenCode plugin, so `opencode --version` must not run")
        },
    );
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("Cursor (this machine, ~/.cursor/hooks.json)"),
        "{lines:?}"
    );
    assert!(
        lines[0].ends_with("not valid JSON. Fix it by hand: Lade does not overwrite it."),
        "{lines:?}"
    );
}

#[test]
fn a_launch_checks_only_the_harness_it_starts() {
    let home = tempfile::tempdir().unwrap();
    let (repo, _) = repo_with_plugin(home.path(), OpenCodeApi::V1.snapshot());
    let cursor = home.path().join(".cursor").join("hooks.json");
    fs::create_dir_all(cursor.parent().unwrap()).unwrap();
    fs::write(&cursor, "{not json").unwrap();

    let lines = report_at(home.path(), &repo, &[Agent::OpenCode], &|| {
        Some(OpenCodeApi::V2)
    });
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("OpenCode (this repo"), "{lines:?}");

    let lines = report_at(
        home.path(),
        &repo,
        &[Agent::Cursor],
        &|| -> Option<OpenCodeApi> {
            panic!("only Cursor starts, so `opencode --version` must not run")
        },
    );
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("Cursor (this machine"), "{lines:?}");
}

#[test]
fn launches_are_known_by_program_name() {
    for (command, agent) in [
        ("opencode", Some(Agent::OpenCode)),
        ("claude --resume", Some(Agent::Claude)),
        ("/opt/homebrew/bin/codex exec fix", Some(Agent::Codex)),
        ("cursor-agent -p hi", Some(Agent::Cursor)),
        ("agent", Some(Agent::Cursor)),
        ("git status", None),
        ("claude-monitor", None),
        ("", None),
    ] {
        assert_eq!(Agent::launched_by(command), agent, "{command:?}");
    }
}
