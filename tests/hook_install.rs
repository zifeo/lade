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
        || body.contains(&format!(r#"["hook", "--harness", "{harness}"]"#))
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
            .args(["hook", "install", "--scope", "user", "--harness", harness])
            .assert()
            .success();
        let user_body = fs::read_to_string(&user).unwrap();
        assert!(
            has_harness_command(&user_body, harness),
            "{harness} user missing harness flag: {user_body}"
        );

        common::lade(home.path())
            .current_dir(dir.path())
            .args([
                "hook",
                "install",
                "--scope",
                "project",
                "--harness",
                harness,
            ])
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
            .args(["hook", "uninstall", "--scope", "user", "--harness", harness])
            .assert()
            .success();
        common::lade(home.path())
            .current_dir(dir.path())
            .args([
                "hook",
                "uninstall",
                "--scope",
                "project",
                "--harness",
                harness,
            ])
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
        .args([
            "hook",
            "install",
            "--scope",
            "project",
            "--harness",
            "claude",
        ])
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
