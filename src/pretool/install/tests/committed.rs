use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::super::agent::Agent;
use super::super::health::report_at;
use super::super::inspect::inspect_at;
use super::super::offer::setup_at;
use super::super::paths::ItemVerb;
use super::super::write::{Scope, refresh_at, teardown_at, write_scoped};

const BLOCKING: &str = r#"{
  "permissions": {"allow": ["Read"]},
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {"type": "command", "command": "lade hook --harness claude", "onFailure": "block"}
        ]
      }
    ]
  }
}
"#;

/// A git repo whose committed `.claude/settings.json` holds `committed`.
fn repo(committed: &str) -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir(repo.path().join(".git")).unwrap();
    let shared = repo.path().join(".claude").join("settings.json");
    fs::create_dir_all(shared.parent().unwrap()).unwrap();
    fs::write(&shared, committed).unwrap();
    (home, repo, shared)
}

fn local(repo: &Path) -> PathBuf {
    repo.join(".claude").join("settings.local.json")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn lade_hooks(value: &Value) -> Vec<Value> {
    value["hooks"]["PreToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entry| entry["hooks"].as_array().unwrap().clone())
        .filter(|hook| {
            hook["command"]
                .as_str()
                .is_some_and(|c| c.contains(" hook"))
        })
        .collect()
}

#[test]
fn enable_copies_the_committed_hook_and_leaves_it_alone() {
    let (home, repo, shared) = repo(BLOCKING);
    let nested = repo.path().join("src");
    fs::create_dir_all(&nested).unwrap();
    let line = write_scoped(Scope::Project, "claude", home.path(), &nested, true).unwrap();
    assert!(line.contains("a copy of the committed"), "{line}");
    assert_eq!(fs::read_to_string(&shared).unwrap(), BLOCKING);
    let copy = read_json(&local(repo.path()));
    assert_eq!(
        lade_hooks(&copy),
        vec![
            json!({"type": "command", "command": "lade hook --harness claude", "onFailure": "block"})
        ]
    );
    assert_eq!(copy["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    let again = write_scoped(Scope::Project, "claude", home.path(), &nested, true).unwrap();
    assert!(again.contains("hook current"), "{again}");
}

#[test]
fn the_copy_takes_the_committed_command_verbatim() {
    let committed = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#;
    let (home, repo, _) = repo(committed);
    write_scoped(Scope::Project, "claude", home.path(), repo.path(), true).unwrap();
    let copy = read_json(&local(repo.path()));
    assert_eq!(
        lade_hooks(&copy),
        vec![json!({"type": "command", "command": "lade hook"})]
    );
}

#[test]
fn enable_rewrites_a_copy_that_lost_on_failure_and_keeps_other_hooks() {
    let (home, repo, _) = repo(BLOCKING);
    fs::write(
        local(repo.path()),
        r#"{"model":"sonnet","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook --harness claude"},{"type":"command","command":"./lint.sh"}]}]}}"#,
    )
    .unwrap();
    write_scoped(Scope::Project, "claude", home.path(), repo.path(), true).unwrap();
    let copy = read_json(&local(repo.path()));
    assert_eq!(copy["model"], "sonnet");
    let hooks = copy["hooks"]["PreToolUse"][0]["hooks"].as_array().unwrap();
    assert!(hooks.iter().any(|hook| hook["command"] == "./lint.sh"));
    assert_eq!(
        lade_hooks(&copy),
        vec![
            json!({"type": "command", "command": "lade hook --harness claude", "onFailure": "block"})
        ]
    );
}

#[test]
fn merge_keeps_on_failure_without_a_committed_hook() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join(".claude")).unwrap();
    fs::write(
        local(repo.path()),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook","onFailure":"block"}]}]}}"#,
    )
    .unwrap();
    write_scoped(Scope::Project, "claude", home.path(), repo.path(), true).unwrap();
    assert_eq!(
        lade_hooks(&read_json(&local(repo.path()))),
        vec![
            json!({"type": "command", "command": "lade hook --harness claude", "onFailure": "block"})
        ]
    );
}

#[test]
fn disable_removes_the_copy_and_says_the_committed_hook_stays() {
    let (home, repo, shared) = repo(BLOCKING);
    write_scoped(Scope::Project, "claude", home.path(), repo.path(), true).unwrap();
    let line = write_scoped(Scope::Project, "claude", home.path(), repo.path(), false).unwrap();
    assert!(line.contains("hook removed from"), "{line}");
    assert!(line.contains("stays: it is the repo's"), "{line}");
    assert_eq!(fs::read_to_string(&shared).unwrap(), BLOCKING);
    assert!(
        !Agent::Claude
            .has_hook(&fs::read_to_string(local(repo.path())).unwrap())
            .unwrap()
    );
}

#[test]
fn teardown_keeps_the_committed_hook() {
    let (home, repo, shared) = repo(BLOCKING);
    write_scoped(Scope::Project, "claude", home.path(), repo.path(), true).unwrap();
    let report = teardown_at(home.path(), repo.path()).unwrap();
    assert!(
        report
            .rows
            .iter()
            .any(|row| row.verb == ItemVerb::Removed && row.path == ".claude/settings.local.json")
    );
    assert!(
        report
            .rows
            .iter()
            .any(|row| row.verb == ItemVerb::Kept && row.path == ".claude/settings.json")
    );
    assert_eq!(fs::read_to_string(&shared).unwrap(), BLOCKING);
}

#[test]
fn setup_copies_the_committed_hook() {
    let (home, repo, shared) = repo(BLOCKING);
    let report = setup_at(false, &["claude"], home.path(), repo.path()).unwrap();
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].verb, ItemVerb::Installed);
    assert!(report.rows[0].note.contains("committed"));
    assert_eq!(fs::read_to_string(&shared).unwrap(), BLOCKING);
    assert_eq!(
        lade_hooks(&read_json(&local(repo.path())))[0]["onFailure"],
        "block"
    );
}

#[test]
fn refresh_follows_the_committed_hook_not_the_snapshot() {
    let committed = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook","onFailure":"block"}]}]}}"#;
    let (home, repo, shared) = repo(committed);
    fs::write(
        local(repo.path()),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook --harness claude"}]}]}}"#,
    )
    .unwrap();
    refresh_at(home.path(), repo.path(), &[Agent::Claude]);
    assert_eq!(
        lade_hooks(&read_json(&local(repo.path()))),
        vec![json!({"type": "command", "command": "lade hook", "onFailure": "block"})]
    );
    assert_eq!(fs::read_to_string(&shared).unwrap(), committed);
}

#[test]
fn refresh_writes_the_copy_the_repo_committed() {
    let (home, repo, shared) = repo(BLOCKING);
    refresh_at(home.path(), repo.path(), &[Agent::Claude]);
    assert_eq!(
        lade_hooks(&read_json(&local(repo.path())))[0]["onFailure"],
        "block"
    );
    assert_eq!(fs::read_to_string(&shared).unwrap(), BLOCKING);
    assert!(report_at(home.path(), repo.path(), &[Agent::Claude], &|| None).is_empty());
}

#[test]
fn refresh_writes_no_copy_over_a_home_hook() {
    let (home, repo, _) = repo(BLOCKING);
    let cwd = tempfile::tempdir().unwrap();
    write_scoped(Scope::User, "claude", home.path(), cwd.path(), true).unwrap();
    refresh_at(home.path(), repo.path(), &[Agent::Claude]);
    assert!(!local(repo.path()).exists());
}

/// A main checkout and a linked worktree whose committed
/// `.claude/settings.json` holds `committed`, laid out as `git worktree add`
/// does. Returns the main checkout and the worktree.
fn worktree(committed: &str) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let main = root.path().canonicalize().unwrap().join("main");
    let wt = root.path().canonicalize().unwrap().join("wt");
    let gitdir = main.join(".git").join("worktrees").join("wt");
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    fs::create_dir_all(wt.join(".claude")).unwrap();
    fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    fs::write(wt.join(".claude").join("settings.json"), committed).unwrap();
    fs::create_dir_all(wt.join("sub")).unwrap();
    (home, root, main, wt)
}

#[test]
fn in_a_worktree_the_copy_goes_to_the_main_checkout() {
    let (home, _root, main, wt) = worktree(BLOCKING);
    let sub = wt.join("sub");
    let lines = report_at(home.path(), &sub, &[Agent::Claude], &|| None);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains(&local(&main).display().to_string()),
        "{lines:?}"
    );

    refresh_at(home.path(), &sub, &[Agent::Claude]);
    assert_eq!(
        lade_hooks(&read_json(&local(&main)))[0]["onFailure"],
        "block"
    );
    assert!(!local(&wt).exists());
    assert!(report_at(home.path(), &sub, &[Agent::Claude], &|| None).is_empty());
    assert!(
        inspect_at(home.path(), &sub)
            .unwrap()
            .claude
            .project
            .current
    );

    teardown_at(home.path(), &sub).unwrap();
    assert!(lade_hooks(&read_json(&local(&main))).is_empty());
}

#[test]
fn a_worktree_root_copy_is_moved_to_the_main_checkout() {
    let (home, _root, main, wt) = worktree(BLOCKING);
    fs::write(
        local(&wt),
        r#"{"permissions":{"allow":["Read"]},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook --harness claude"}]}]}}"#,
    )
    .unwrap();
    let line = write_scoped(Scope::Project, "claude", home.path(), &wt, true).unwrap();
    assert!(line.contains(&local(&main).display().to_string()), "{line}");
    let stale = read_json(&local(&wt));
    assert_eq!(stale["permissions"]["allow"], json!(["Read"]));
    assert!(
        !fs::read_to_string(local(&wt))
            .unwrap()
            .contains("lade hook")
    );
    assert_eq!(
        lade_hooks(&read_json(&local(&main)))[0]["onFailure"],
        "block"
    );
}

#[test]
fn health_reports_a_missing_or_differing_copy() {
    let (home, repo, _) = repo(BLOCKING);
    let lines = report_at(home.path(), repo.path(), &[Agent::Claude], &|| None);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("no copy of the committed hook"),
        "{lines:?}"
    );

    fs::write(
        local(repo.path()),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook --harness claude"}]}]}}"#,
    )
    .unwrap();
    let lines = report_at(home.path(), repo.path(), &[Agent::Claude], &|| None);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].contains("differs from the committed one"),
        "{lines:?}"
    );

    write_scoped(Scope::Project, "claude", home.path(), repo.path(), true).unwrap();
    assert!(report_at(home.path(), repo.path(), &[Agent::Claude], &|| None).is_empty());
}

#[test]
fn health_warns_when_a_home_hook_stacks_on_the_committed_one() {
    let (home, repo, _) = repo(BLOCKING);
    let cwd = tempfile::tempdir().unwrap();
    write_scoped(Scope::User, "claude", home.path(), cwd.path(), true).unwrap();
    let lines = report_at(home.path(), repo.path(), &[Agent::Claude], &|| None);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("Both will run"), "{lines:?}");
}

#[test]
fn health_reports_an_unreadable_committed_file() {
    let (home, repo, shared) = repo("{not json");
    let lines = report_at(home.path(), repo.path(), &[Agent::Claude], &|| None);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("not valid JSON"), "{lines:?}");
    let err = write_scoped(Scope::Project, "claude", home.path(), repo.path(), true).unwrap_err();
    assert!(format!("{err:#}").contains("not valid JSON"), "{err:#}");
    assert_eq!(fs::read_to_string(&shared).unwrap(), "{not json");
}

#[test]
fn inspect_reports_the_committed_hook() {
    let (home, repo, _) = repo(BLOCKING);
    let status = inspect_at(home.path(), repo.path()).unwrap();
    let committed = status.claude.committed.as_ref().unwrap();
    assert!(committed.present && committed.blocking);
    assert!(!status.claude.project.installed);
    assert!(status.cursor.committed.is_none());

    fs::write(
        local(repo.path()),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook --harness claude"}]}]}}"#,
    )
    .unwrap();
    let status = inspect_at(home.path(), repo.path()).unwrap();
    assert!(status.claude.project.installed && !status.claude.project.current);

    write_scoped(Scope::Project, "claude", home.path(), repo.path(), true).unwrap();
    let status = inspect_at(home.path(), repo.path()).unwrap();
    assert!(status.claude.project.current);
}

#[test]
fn a_committed_hook_without_on_failure_is_not_blocking() {
    let committed = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook --harness claude"}]}]}}"#;
    let (home, repo, _) = repo(committed);
    let status = inspect_at(home.path(), repo.path()).unwrap();
    let committed = status.claude.committed.as_ref().unwrap();
    assert!(committed.present && !committed.blocking);
}
