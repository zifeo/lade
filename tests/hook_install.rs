mod common;

use std::fs;
use tempfile::tempdir;

fn status_pretool(home: &std::path::Path, cwd: &std::path::Path) -> serde_json::Value {
    let output = common::lade(home)
        .current_dir(cwd)
        .args(["status", "--json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    value["hooks"]["pretool"].clone()
}

fn has_harness_command(body: &str, harness: &str) -> bool {
    body.contains(&format!("lade hook --harness {harness}"))
        || body.contains(&format!(r#""--harness", "{harness}"]"#))
}

/// PATH with a stub `opencode` that prints `version`, ahead of the real one.
#[cfg(unix)]
fn path_with_opencode(bin: &std::path::Path, version: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(bin).unwrap();
    let stub = bin.join("opencode");
    fs::write(&stub, format!("#!/bin/sh\necho '{version}'\n")).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

#[test]
fn hook_install_and_uninstall_cover_both_scopes() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    let path = path_with_opencode(&home.path().join("bin"), "opencode v2.0.29");

    let cases = [
        (
            "cursor",
            home.path().join(".cursor").join("hooks.json"),
            dir.path().join(".cursor").join("hooks.json"),
        ),
        (
            "claude",
            home.path().join(".claude").join("settings.json"),
            dir.path().join(".claude").join("settings.json"),
        ),
        (
            "codex",
            home.path().join(".codex").join("hooks.json"),
            dir.path().join(".codex").join("hooks.json"),
        ),
        (
            "opencode",
            home.path()
                .join(".config")
                .join("opencode")
                .join("plugins")
                .join("lade-pretool.js"),
            dir.path()
                .join(".opencode")
                .join("plugins")
                .join("lade-pretool.js"),
        ),
    ];

    for (harness, user, project) in cases {
        common::lade(home.path())
            .current_dir(dir.path())
            .env("PATH", &path)
            .args(["hook", "enable", "--global", "--harness", harness])
            .assert()
            .success();
        let user_body = fs::read_to_string(&user).unwrap();
        assert!(
            has_harness_command(&user_body, harness),
            "{harness} user missing harness flag: {user_body}"
        );

        common::lade(home.path())
            .current_dir(dir.path())
            .env("PATH", &path)
            .args(["hook", "enable", "--harness", harness])
            .assert()
            .success();
        let project_body = fs::read_to_string(&project).unwrap();
        assert!(
            has_harness_command(&project_body, harness),
            "{harness} project missing harness flag: {project_body}"
        );

        let pretool = status_pretool(home.path(), dir.path());
        assert_eq!(pretool[harness]["global"]["installed"], true);
        assert_eq!(pretool[harness]["global"]["current"], true);
        assert_eq!(pretool[harness]["project"]["installed"], true);
        assert_eq!(pretool[harness]["project"]["current"], true);

        common::lade(home.path())
            .current_dir(dir.path())
            .env("PATH", &path)
            .args(["hook", "disable", "--global", "--harness", harness])
            .assert()
            .success();
        common::lade(home.path())
            .current_dir(dir.path())
            .env("PATH", &path)
            .args(["hook", "disable", "--harness", harness])
            .assert()
            .success();

        let pretool = status_pretool(home.path(), dir.path());
        assert_eq!(pretool[harness]["global"]["installed"], false);
        assert_eq!(pretool[harness]["project"]["installed"], false);
    }
}

#[test]
fn hook_install_preserves_claude_permissions_and_model() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    let path = dir.path().join(".claude").join("settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        r#"{"permissions":{"allow":["Read"]},"model":"sonnet"}
"#,
    )
    .unwrap();

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "enable", "--harness", "claude"])
        .assert()
        .success();

    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["permissions"]["allow"][0], "Read");
    assert_eq!(value["model"], "sonnet");
    let command = value["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(
        has_harness_command(command, "claude"),
        "unexpected command: {command}"
    );
}

#[cfg(unix)]
#[test]
fn opencode_plugin_follows_the_installed_opencode() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    let plugin = dir
        .path()
        .join(".opencode")
        .join("plugins")
        .join("lade-pretool.js");
    let enable = |path: &str| {
        common::lade(home.path())
            .current_dir(dir.path())
            .env("PATH", path)
            .args(["hook", "enable", "--harness", "opencode"])
            .assert()
    };

    enable(&path_with_opencode(&home.path().join("v1"), "1.19.0")).success();
    let body = fs::read_to_string(&plugin).unwrap();
    assert!(body.contains("export const LadePretool"), "{body}");

    enable(&path_with_opencode(
        &home.path().join("v2"),
        "opencode v2.0.29",
    ))
    .success();
    let body = fs::read_to_string(&plugin).unwrap();
    assert!(body.contains(r#"id: "lade-pretool""#), "{body}");
    assert!(!body.contains("LadePretool"), "{body}");

    enable("/usr/bin:/bin")
        .failure()
        .stderr(predicates::str::contains("`opencode` is not on PATH"));
    assert!(
        fs::read_to_string(&plugin)
            .unwrap()
            .contains(r#"id: "lade-pretool""#)
    );
}
