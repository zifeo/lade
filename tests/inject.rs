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
fn test_inject_no_mask_shows_raw_secret() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  SECRET: rawsecret99\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("rawsecret99"));
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

#[test]
fn test_inject_stdin_escape_responses_do_not_leak_to_stdout() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  SECRET: stdincheck42\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "echo hello"])
        .write_stdin("\x1b]11;rgb:1f1f/2424/2828\x1b\\\x1b[37;1R")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("hello"), "expected child output: {stdout}");
    assert!(
        !stdout.contains("\x1b]11;"),
        "OSC 11 response leaked into stdout: {stdout:?}"
    );
    assert!(
        !stdout.contains("\x1b[37;1R"),
        "CPR response leaked into stdout: {stdout:?}"
    );
}

#[test]
fn test_inject_exit_code_propagation() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "exit 7"])
        .assert()
        .code(7);
}

#[test]
fn test_alias_exit_code_propagation() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["exit", "7"])
        .assert()
        .code(7);
}

#[test]
fn test_alias_with_separator() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo.*\":\n  SECRET: separator_secret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["--", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("separator_secret"));
}

#[test]
fn test_inject_network_provider_error_is_boxed() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"curl.*\":\n  DB_PORT: kubectl://bad-host/dev/service/postgres/5432\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "curl http://127.0.0.1:18080/"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "Lade could not start network providers:",
        ))
        .stderr(predicates::str::contains("network provider error:"));
}

#[test]
fn test_inject_network_parse_error_is_boxed() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"curl.*\":\n  DB_PORT: kubectl://k8s.example.com:6443\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "curl http://127.0.0.1:18080/"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "Lade could not start network providers:",
        ))
        .stderr(predicates::str::contains("missing"));
}
