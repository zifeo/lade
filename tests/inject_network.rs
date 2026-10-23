mod common;
use std::fs;
use tempfile::tempdir;

fn seed_kubectl(installs: &std::path::Path) {
    let dest_dir = installs.join("kubectl/1.31.4");
    fs::create_dir_all(&dest_dir).unwrap();
    let dest = dest_dir.join("kubectl");
    fs::write(&dest, "#!/bin/sh\necho missing >&2; exit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn test_inject_network_provider_error_is_boxed() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = tempdir().unwrap();
    seed_kubectl(installs.path());
    fs::write(
        dir.path().join("lade.yml"),
        "\"curl.*\":\n  DB_PORT: kubectl://bad-host/dev/service/postgres/5432\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", installs.path())
        .args(["inject", "curl http://127.0.0.1:18080/"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Could not start tunnels:"))
        .stderr(predicates::str::contains("tunnel error:"));
}

#[test]
fn test_inject_network_refuses_a_port_already_in_use() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    fs::write(
        dir.path().join("lade.yml"),
        format!("\"curl.*\":\n  {port}: ssh://jump.example.com:22/db.internal/5432\n"),
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "curl http://127.0.0.1:18080/"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Could not start tunnels:"))
        .stderr(predicates::str::contains(format!(
            "Local port {port} is already in use"
        )));
}

#[test]
fn test_inject_network_parse_error_is_boxed() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = tempdir().unwrap();
    seed_kubectl(installs.path());
    fs::write(
        dir.path().join("lade.yml"),
        "\"curl.*\":\n  DB_PORT: kubectl://k8s.example.com:6443\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", installs.path())
        .args(["inject", "curl http://127.0.0.1:18080/"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Could not start tunnels:"))
        .stderr(predicates::str::contains("missing"));
}
