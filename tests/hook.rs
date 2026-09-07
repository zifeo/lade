mod common;

use tempfile::tempdir;

#[test]
fn unread_hook_payload_is_visible_on_stderr() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin("not json at all")
        .assert()
        .success()
        .stdout("")
        .stderr(predicates::str::contains("hook stdin is not JSON"))
        .stderr(predicates::str::contains(
            "The command will run without injection.",
        ));
}

#[test]
fn hook_event_without_command_is_visible_on_stderr() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin(r#"{"tool_name":"Bash","hook_event_name":"PreToolUse"}"#)
        .assert()
        .success()
        .stdout("")
        .stderr(predicates::str::contains("no command to wrap"))
        .stderr(predicates::str::contains(
            "The command will run without injection.",
        ));
}
