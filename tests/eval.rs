mod common;

use predicates::prelude::*;

#[test]
fn eval_prints_file_uri_and_writes_diary() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("secret.json");
    std::fs::write(&key, r#"{"token":"eval-secret-value"}"#).unwrap();
    let uri = format!("file://{}?query=.token", key.display());

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["eval", &uri])
        .assert()
        .success()
        .stdout(predicate::str::contains("eval-secret-value"));

    let rows = common::log_rows(home.path(), dir.path());
    let items = rows.as_array().expect("log array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "access");
    assert_eq!(items[0]["command"], "eval");
    let dump = serde_json::to_string(&items[0]).unwrap();
    assert!(dump.contains(&uri), "{dump}");
    assert!(
        !dump.contains("eval-secret-value"),
        "diary stored the secret: {dump}"
    );
}

#[test]
fn eval_unknown_uri_is_literal() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["eval", "not-a-scheme"])
        .assert()
        .success()
        .stdout(predicate::eq("not-a-scheme\n"));
}
