use super::super::handle;
use super::{AGENT_ENV, with_ticket_tmpdir};
use crate::config::{Audience, Config, LadeFile};
use crate::ticket::{self, PreEvent};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tempfile::{TempDir, tempdir};

/// A repo whose root injects into every command, with a `^tool` rule one
/// directory down: the layout that kept the subdirectory rule unloaded.
fn repo() -> (TempDir, PathBuf, Config) {
    let root = tempdir().unwrap();
    std::fs::write(root.path().join("lade.yml"), "\".\":\n  ROOT: val\n").unwrap();
    let sub = root.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("lade.yml"), "\"^tool\":\n  SUB: val\n").unwrap();
    let config = LadeFile::build(root.path().to_path_buf()).unwrap();
    (root, sub, config)
}

fn claude(command: &str, cwd: &Path) -> String {
    json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": cwd,
    })
    .to_string()
}

fn run(config: &Config, input: &str, harness: &str) -> String {
    let mut out = String::new();
    temp_env::with_vars(AGENT_ENV, || {
        out = handle(config, input, Audience::Agent, Some(harness)).unwrap();
    });
    out
}

fn rewritten(result: &str) -> String {
    let parsed: Value = serde_json::from_str(result).unwrap_or_else(|_| panic!("{result:?}"));
    parsed["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap_or_else(|| panic!("{result}"))
        .to_string()
}

/// The ticket behind `lade --pretool=<id> '<command>'`, after checking the
/// whole command is wrapped unchanged.
fn ticket_of(result: &str, command: &str) -> PreEvent {
    let cmd = rewritten(result);
    let id = cmd
        .split("--pretool=")
        .nth(1)
        .and_then(|rest| rest.get(..4))
        .unwrap_or_else(|| panic!("{cmd}"));
    assert!(
        cmd.ends_with(&format!(" --pretool={id} '{command}' && true")),
        "{cmd}"
    );
    ticket::read(id).unwrap()
}

fn rules(pre: &PreEvent) -> Vec<(String, String)> {
    pre.matches
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["file"].as_str().unwrap().to_string(),
                m["rule"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn has_sub_rule(pre: &PreEvent, sub: &Path) -> bool {
    rules(pre).contains(&(sub.to_string_lossy().into_owned(), "^tool".to_string()))
}

#[test]
fn bare_moves_are_not_wrapped() {
    with_ticket_tmpdir(|| {
        let (root, _sub, config) = repo();
        for command in [
            "cd sub",
            "  cd sub  ",
            "cd",
            "cd -",
            "cd ..",
            "cd /tmp",
            "cd ~/src/app",
            "pushd sub",
            "pushd",
            "popd",
        ] {
            let result = run(&config, &claude(command, root.path()), "claude");
            assert_eq!(result, "", "{command}: {result}");
        }
    });
}

#[test]
fn bare_move_through_a_vehicle_is_peeled_not_wrapped() {
    with_ticket_tmpdir(|| {
        let (root, _sub, config) = repo();
        let result = run(
            &config,
            &claude("lade inject -- cd sub", root.path()),
            "claude",
        );
        assert_eq!(rewritten(&result), "cd sub");
    });
}

#[test]
fn compound_moves_stay_wrapped() {
    with_ticket_tmpdir(|| {
        let (root, _sub, config) = repo();
        for command in [
            "cd sub && tool",
            "cd sub; ls",
            "cd sub && cd ..",
            "cd sub | cat",
            "cd sub\nls",
            "cd $(mktemp -d)",
            "cd `pwd`",
            "cd -P sub",
            "FOO=1 cd sub",
        ] {
            let result = run(&config, &claude(command, root.path()), "claude");
            assert!(
                rewritten(&result).contains(" --pretool="),
                "{command}: {result}"
            );
        }
    });
}

#[test]
fn leading_cd_loads_the_rules_of_that_directory() {
    with_ticket_tmpdir(|| {
        let (root, sub, config) = repo();
        let absolute = format!("cd {} && tool plan", sub.display());
        for command in [
            "cd sub && tool plan",
            "cd sub&&tool plan",
            "cd ./sub/ && tool plan",
            "cd sub/.. && cd sub && tool plan",
            absolute.as_str(),
        ] {
            let result = run(&config, &claude(command, root.path()), "claude");
            let pre = ticket_of(&result, command);
            assert!(has_sub_rule(&pre, &sub), "{command}: {:?}", rules(&pre));
            assert_eq!(pre.cwd, sub, "{command}");
            assert_eq!(pre.command, command);
        }
    });
}

#[test]
fn leading_cd_expands_the_home_tilde() {
    with_ticket_tmpdir(|| {
        let (root, sub, config) = repo();
        let command = "cd ~/sub && tool";
        temp_env::with_var("HOME", Some(root.path()), || {
            let result = run(&config, &claude(command, root.path()), "claude");
            let pre = ticket_of(&result, command);
            assert!(has_sub_rule(&pre, &sub), "{:?}", rules(&pre));
            assert_eq!(pre.cwd, sub);
        });
    });
}

#[test]
fn unresolvable_cd_keeps_the_last_resolved_directory() {
    with_ticket_tmpdir(|| {
        let (root, sub, config) = repo();
        for (command, cwd) in [
            ("cd $DIR && tool", root.path().to_path_buf()),
            ("cd missing && tool", root.path().to_path_buf()),
            ("cd \"sub\" && tool", root.path().to_path_buf()),
            ("cd sub; tool", root.path().to_path_buf()),
            ("cd sub && cd $DIR && tool", sub.clone()),
        ] {
            let result = run(&config, &claude(command, root.path()), "claude");
            let pre = ticket_of(&result, command);
            assert!(!has_sub_rule(&pre, &sub), "{command}: {:?}", rules(&pre));
            assert_eq!(pre.cwd, cwd, "{command}");
        }
    });
}

#[test]
fn claude_cwd_loads_the_rules_of_that_directory() {
    with_ticket_tmpdir(|| {
        let (_root, sub, config) = repo();
        let result = run(&config, &claude("tool", &sub), "claude");
        let pre = ticket_of(&result, "tool");
        assert!(has_sub_rule(&pre, &sub), "{:?}", rules(&pre));
        assert_eq!(pre.cwd, sub);
    });
}

#[test]
fn codex_workdir_is_relative_to_the_session_cwd() {
    with_ticket_tmpdir(|| {
        let (root, sub, config) = repo();
        let input = json!({
            "hook_event_name": "PreToolUse",
            "turn_id": "t1",
            "tool_name": "Bash",
            "tool_input": {"command": "tool", "workdir": "sub"},
            "cwd": root.path(),
        })
        .to_string();
        let pre = ticket_of(&run(&config, &input, "codex"), "tool");
        assert!(has_sub_rule(&pre, &sub), "{:?}", rules(&pre));
        assert_eq!(pre.cwd, sub);
    });
}

#[test]
fn cursor_working_directory_loads_the_rules_of_that_directory() {
    with_ticket_tmpdir(|| {
        let (root, sub, config) = repo();
        let input = json!({
            "hook_event_name": "preToolUse",
            "tool_name": "Shell",
            "tool_input": {"command": "tool", "working_directory": sub},
            "cwd": root.path(),
        })
        .to_string();
        let result = run(&config, &input, "cursor");
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let cmd = parsed["updated_input"]["command"].as_str().unwrap();
        let id = &cmd.split("--pretool=").nth(1).unwrap()[..4];
        let pre = ticket::read(id).unwrap();
        assert!(has_sub_rule(&pre, &sub), "{:?}", rules(&pre));
        assert_eq!(pre.cwd, sub);
        assert_eq!(parsed["updated_input"]["working_directory"], json!(sub));
    });
}

#[test]
fn without_a_payload_directory_the_config_directory_is_used() {
    with_ticket_tmpdir(|| {
        let (root, sub, config) = repo();
        let input = json!({"command": "tool", "session_id": "s1"}).to_string();
        let result = run(&config, &input, "opencode");
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let cmd = parsed["command"].as_str().unwrap();
        let id = &cmd.split("--pretool=").nth(1).unwrap()[..4];
        let pre = ticket::read(id).unwrap();
        assert!(!has_sub_rule(&pre, &sub), "{:?}", rules(&pre));
        assert_eq!(pre.cwd, root.path());
    });
}

#[test]
fn cdpath_leaves_a_searched_cd_to_the_shell() {
    let root = tempdir().unwrap();
    std::fs::create_dir(root.path().join("sub")).unwrap();
    temp_env::with_var("CDPATH", Some("/elsewhere"), || {
        let (dir, rest) = super::super::run_dir(root.path(), "cd sub && tool");
        assert_eq!((dir.as_path(), rest), (root.path(), "cd sub && tool"));
        let (dir, rest) = super::super::run_dir(root.path(), "cd ./sub && tool");
        assert_eq!((dir, rest), (root.path().join("sub"), "tool"));
    });
}

#[cfg(unix)]
#[test]
fn a_symlinked_directory_is_the_same_directory() {
    let root = tempdir().unwrap();
    let real = root.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(super::super::same_dir(&link, &real));
    assert!(!super::super::same_dir(&link, root.path()));
}
