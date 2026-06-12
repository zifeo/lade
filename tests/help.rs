mod common;
use tempfile::tempdir;

#[test]
fn test_help() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("-h")
        .assert()
        .success();
}

#[test]
fn test_status_subcommand_is_not_treated_as_alias() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .code(1)
        .stdout(predicates::str::contains("lade version:"));
}

#[test]
fn test_top_level_no_mask_remains_invalid() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["--no-mask", "echo", "hello"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unexpected argument '--no-mask'"));
}
