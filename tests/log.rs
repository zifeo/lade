mod common;
use tempfile::tempdir;

#[test]
fn log_is_not_inject_alias() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["log"])
        .assert()
        .success();
}

#[test]
fn log_help_prints_db_path_and_duration() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let db = home.path().join("events.db");
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains(db.to_string_lossy().as_ref()))
        .stdout(predicates::str::contains("Nmonth"));
}
