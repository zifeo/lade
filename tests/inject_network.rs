mod common;
use std::fs;
use tempfile::tempdir;

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
