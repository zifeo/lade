mod common;

use tempfile::tempdir;

#[test]
fn test_user_set() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["user", "testuser"])
        .assert()
        .success()
        .stdout(predicates::str::contains("testuser"));
}

#[test]
fn test_user_get_after_set() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["user", "testuser"])
        .assert()
        .success();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["user"])
        .assert()
        .success()
        .stdout(predicates::str::contains("testuser"));
}

#[test]
fn test_user_reset() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["user", "testuser"])
        .assert()
        .success();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["user", "--reset"])
        .assert()
        .success()
        .stdout(predicates::str::contains("reset"));
}

#[test]
fn test_user_get_no_user_set() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["user"])
        .assert()
        .success()
        .stdout(predicates::str::contains("No user set"));
}
