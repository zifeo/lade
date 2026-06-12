mod common;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_status_reports_version_and_project() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .stdout(predicates::str::contains("lade version:"))
        .stdout(predicates::str::contains("project config: ok"));
}
