mod common;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_inject_raw_value_reaches_child_process() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  SECRET: injected_secret\n",
    )
    .unwrap();
    // --no-mask: verify the secret is actually injected into the child env.
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("injected_secret"));
}

#[test]
fn test_alias_raw_value_reaches_child_process() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  SECRET: injected_secret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("injected_secret"));
}

#[test]
fn test_inject_masks_loader_secret_in_output_by_default() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let source = dir.path().join("source.json");
    fs::write(&source, r#"{"token":"loadersecret42"}"#).unwrap();
    let source_url_path = source.to_str().unwrap().replace('\\', "/");
    let lade_yml = format!(
        "\"echo.*\":\n  SECRET: \"file://{}?query=.token\"\n",
        source_url_path
    );
    fs::write(dir.path().join("lade.yml"), &lade_yml).unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "echo", "$SECRET"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(
        !stdout.contains("loadersecret42"),
        "loader secret leaked into output: {stdout}"
    );
    assert!(
        stdout.contains("${SECRET:-REDACTED}"),
        "expected redaction token in output: {stdout}"
    );
}

#[test]
fn test_alias_masks_loader_secret_in_output_by_default() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let source = dir.path().join("source.json");
    fs::write(&source, r#"{"token":"loadersecret42"}"#).unwrap();
    let source_url_path = source.to_str().unwrap().replace('\\', "/");
    let lade_yml = format!(
        "\"echo.*\":\n  SECRET: \"file://{}?query=.token\"\n",
        source_url_path
    );
    fs::write(dir.path().join("lade.yml"), &lade_yml).unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["echo", "$SECRET"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(
        !stdout.contains("loadersecret42"),
        "loader secret leaked into output: {stdout}"
    );
    assert!(
        stdout.contains("${SECRET:-REDACTED}"),
        "expected redaction token in output: {stdout}"
    );
}

#[test]
fn test_inject_does_not_mask_raw_literal_in_output() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  VERSION: \"3\"\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args([
            "inject",
            "echo",
            "uuid c4ba23e1-e702-4774-b7ce-0cf6952e5030",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(
        stdout.contains("c4ba23e1-e702-4774-b7ce-0cf6952e5030"),
        "digits from unrelated output must not be redacted: {stdout}"
    );
    assert!(
        !stdout.contains("REDACTED"),
        "raw literals must not be masked in output: {stdout}"
    );
}

#[test]
fn test_inject_static_mask_format() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let source = dir.path().join("source.json");
    fs::write(&source, r#"{"token":"statictest77"}"#).unwrap();
    let source_url_path = source.to_str().unwrap().replace('\\', "/");
    let lade_yml = format!(
        "\"echo.*\":\n  SECRET: \"file://{}?query=.token\"\n",
        source_url_path
    );
    fs::write(dir.path().join("lade.yml"), &lade_yml).unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--mask-format", "REDACTED", "echo", "$SECRET"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(
        !stdout.contains("statictest77"),
        "loader secret leaked: {stdout}"
    );
    assert!(stdout.contains("REDACTED"), "expected REDACTED: {stdout}");
    assert!(
        !stdout.contains("SECRET"),
        "var name should not appear with static format: {stdout}"
    );
}

/// The child prints nothing: whatever holds the value is Lade's own output.
#[test]
fn debug_logs_hold_no_resolved_value() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let source = dir.path().join("source.json");
    fs::write(&source, format!(r#"{{"token":"{}"}}"#, common::SECRET)).unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"^true\":\n  KEY: \"file://{}?query=.token\"\n",
            source.display()
        ),
    )
    .unwrap();
    for (args, log) in [
        (&["-vvv", "inject", "true"][..], ""),
        (&["inject", "true"][..], "trace"),
    ] {
        let out = common::lade(home.path())
            .current_dir(dir.path())
            .env("LADE_LOG", log)
            .args(args)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{stderr}");
        assert!(
            stderr.contains("KEY") || stderr.contains("hydrate"),
            "no log: {stderr}"
        );
        assert!(!stderr.contains(common::SECRET), "{stderr}");
        assert!(!String::from_utf8_lossy(&out.stdout).contains(common::SECRET));
    }
}

/// A `lade.yml` whose rule reads `common::SECRET` through the file provider.
fn file_secret_uri(dir: &std::path::Path) -> String {
    let source = dir.join("source.json");
    fs::write(&source, format!(r#"{{"token":"{}"}}"#, common::SECRET)).unwrap();
    format!("file://{}?query=.token", source.display())
}

fn inject_stdout(home: &std::path::Path, dir: &std::path::Path, args: &[&str]) -> String {
    let out = common::lade(home)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn private_and_composed_values_are_masked() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let uri = file_secret_uri(dir.path());
    fs::write(
        dir.path().join("lade.yml"),
        format!("\"^echo\":\n  .TOKEN: \"{uri}\"\n  AUTH: \"Bearer ${{TOKEN}}\"\n"),
    )
    .unwrap();
    let stdout = inject_stdout(
        home.path(),
        dir.path(),
        &[
            "inject",
            "echo",
            "[$AUTH]",
            &format!("[{}]", common::SECRET),
        ],
    );
    assert!(!stdout.contains(common::SECRET), "{stdout}");
    assert!(stdout.contains("[${AUTH:-REDACTED}]"), "{stdout}");
    assert!(stdout.contains("[${TOKEN:-REDACTED}]"), "{stdout}");
}

#[test]
fn file_output_is_masked_and_removed() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let uri = file_secret_uri(dir.path());
    fs::write(
        dir.path().join("lade.yml"),
        format!("\"^cat\":\n  .:\n    file: out.json\n  KEY: \"{uri}\"\n"),
    )
    .unwrap();
    let stdout = inject_stdout(home.path(), dir.path(), &["inject", "cat", "out.json"]);
    assert!(!stdout.contains(common::SECRET), "{stdout}");
    assert!(stdout.contains("${KEY:-REDACTED}"), "{stdout}");
    assert!(!dir.path().join("out.json").exists());
}

#[test]
fn test_inject_no_mask_prints_the_value() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let uri = file_secret_uri(dir.path());
    fs::write(
        dir.path().join("lade.yml"),
        format!("\"^echo\":\n  KEY: \"{uri}\"\n"),
    )
    .unwrap();
    let masked = inject_stdout(home.path(), dir.path(), &["inject", "echo", "$KEY"]);
    assert!(!masked.contains(common::SECRET), "{masked}");
    let shown = inject_stdout(
        home.path(),
        dir.path(),
        &["inject", "--no-mask", "echo", "$KEY"],
    );
    assert!(shown.contains(common::SECRET), "{shown}");
}

#[test]
fn pretool_wrap_masks() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let uri = file_secret_uri(dir.path());
    fs::write(
        dir.path().join("lade.yml"),
        format!("\"^echo\":\n  KEY: \"{uri}\"\n"),
    )
    .unwrap();
    let stdout = inject_stdout(home.path(), dir.path(), &["--pretool", "echo", "$KEY"]);
    assert!(!stdout.contains(common::SECRET), "{stdout}");
    assert!(stdout.contains("${KEY:-REDACTED}"), "{stdout}");
}

#[test]
fn cached_binding_still_feeds_its_dependent() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let source = dir.path().join("source.json");
    fs::write(&source, r#"{"token":"first-value"}"#).unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"^echo\":\n  A: \"file://{}?query=.token\"\n  B: \"Bearer ${{A}}\"\n  C: \"sh://printf %s \\\"$A\\\"\"\n",
            source.display()
        ),
    )
    .unwrap();
    let args = ["inject", "--no-mask", "echo", "[$B]", "[$C]"];
    let first = inject_stdout(home.path(), dir.path(), &args);
    assert!(
        first.contains("[Bearer first-value] [first-value]"),
        "{first}"
    );
    // The second run must take A from the hub, not the file.
    fs::write(&source, r#"{"token":"second-value"}"#).unwrap();
    let second = inject_stdout(home.path(), dir.path(), &args);
    assert!(
        second.contains("[Bearer first-value] [first-value]"),
        "{second}"
    );
}

#[test]
fn parent_yaml_parse_error_stops_command() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let child = root.path().join("app");
    fs::create_dir(&child).unwrap();
    fs::write(root.path().join("lade.yml"), "\"^echo\": [\n").unwrap();
    fs::write(child.join("lade.yml"), "\"^echo\":\n  KEY: v\n").unwrap();
    for args in [&["inject", "echo", "ran"][..], &["set", "echo ran"][..]] {
        let out = common::lade(home.path())
            .current_dir(&child)
            .env("HOME", root.path())
            .args(args)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{args:?} {stderr}");
        assert!(
            stderr.contains("Could not parse a lade.yaml."),
            "{args:?} {stderr}"
        );
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("ran"),
            "{args:?}"
        );
    }
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": "echo ran" },
        "hook_event_name": "PreToolUse",
        "cwd": child,
    });
    let out = common::lade(home.path())
        .current_dir(&child)
        .env("HOME", root.path())
        .args(["hook", "--harness", "claude"])
        .write_stdin(payload.to_string())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "hook {stderr}");
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        answer["hookSpecificOutput"]["permissionDecision"], "deny",
        "hook {answer}"
    );
    assert!(
        stderr.contains("Could not load a lade.yaml"),
        "hook {stderr}"
    );
}
