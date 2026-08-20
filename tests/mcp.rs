mod common;

use std::fs;

use predicates::prelude::PredicateBooleanExt;
use tempfile::tempdir;

#[test]
fn test_mcp_stdio_injects_public_bindings_only() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^env$\":\n  .TOKEN: hidden\n  PUBLIC: \"Bearer ${TOKEN}\"\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["mcp", "--", "env"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PUBLIC=Bearer hidden"))
        .stdout(predicates::str::contains("TOKEN=hidden").not());
}

#[test]
fn test_mcp_agent_when_uses_env_signal() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^env$\":\n  \".\":\n    when: agent\n  PUBLIC: agentsecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["mcp", "--", "env"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PUBLIC=agentsecret").not());
    common::lade(home.path())
        .current_dir(dir.path())
        .env("CURSOR_AGENT", "1")
        .args(["mcp", "--", "env"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PUBLIC=agentsecret"));
}

#[test]
fn test_mcp_requires_one_transport_target() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("mcp")
        .assert()
        .failure()
        .stderr(predicates::str::contains("provide an MCP HTTPS URL"));
}
