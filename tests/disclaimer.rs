mod common;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_disclaimer_hook_flow() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"deploy\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();

    // 1. set blocked
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "deploy"])
        .assert()
        .failure()
        .stdout(predicates::str::contains("unset -v LADE_PENDING"))
        .stdout(predicates::str::contains("export LADE_PENDING='v1:"))
        .stderr(predicates::str::contains("lade: disclaimer required"));

    // 2. set with env bypass
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_ACCEPT_DISCLAIMER", "1")
        .args(["set", "deploy"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export SECRET='val'"));

    // 3. approve without pending
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("approve")
        .assert()
        .failure()
        .stderr(predicates::str::contains("no pending disclaimer"));
}
