#![cfg(unix)]

mod common;
use tempfile::tempdir;

#[test]
fn inject_keeps_path_through_a_shim() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let shims = tempdir().unwrap();
    // Like a mise shim found first on PATH: rewrites PATH, then runs the shell.
    common::fake_cli(&shims, "bash", "PATH=/usr/bin:/bin exec /bin/sh \"$@\"");
    let marker = "/opt/acme/pinned/bin";
    let path = format!(
        "{}:{marker}:{}",
        shims.path().display(),
        std::env::var("PATH").unwrap()
    );

    common::lade(home.path())
        .current_dir(dir.path())
        .env("PATH", &path)
        .args(["inject", "--no-mask", "printenv", "PATH"])
        .assert()
        .success()
        .stdout(predicates::str::starts_with(path));
}
