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

/// bash and zsh set `_` to the path of each program they start; a program
/// Lade starts itself inherits whatever `_` Lade had.
fn started_by_a_shell(out: &[u8]) -> bool {
    String::from_utf8_lossy(out)
        .lines()
        .any(|line| line.starts_with("_=") && line.ends_with("/env"))
}

#[test]
fn every_line_runs_in_the_shell() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    for args in [
        vec!["inject", "--no-mask", "env"],
        vec!["--pretool", "--", "env"],
    ] {
        let out = common::lade(home.path())
            .current_dir(dir.path())
            .args(&args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert!(started_by_a_shell(&out), "{args:?}");
    }
}

#[test]
fn a_line_the_shell_must_read_still_runs_in_it() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    for (line, shown) in [
        ("FOO=lade-marker env", "FOO=lade-marker"),
        ("env | grep -c ^PATH=", "1"),
        ("printenv HOME #comment", home.path().to_str().unwrap()),
        ("echo lade-marker", "lade-marker"),
    ] {
        common::lade(home.path())
            .current_dir(dir.path())
            .args(["inject", "--no-mask", line])
            .assert()
            .success()
            .stdout(predicates::str::contains(shown));
    }
}

#[test]
fn a_script_without_shebang_falls_back_to_the_shell() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let script = dir.path().join("hello");
    std::fs::write(&script, "echo from-script\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--no-mask", "./hello"])
        .assert()
        .success()
        .stdout("from-script\n");
}

#[test]
fn a_direct_child_keeps_its_exit_code_and_argv0() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let bin = tempdir().unwrap();
    common::fake_cli(&bin, "probe", "echo \"$0 $*\"; exit 7");
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap()
    );
    common::lade(home.path())
        .current_dir(dir.path())
        .env("PATH", &path)
        .args(["inject", "--no-mask", "probe", "a", "b"])
        .assert()
        .code(7)
        .stdout(format!("{}/probe a b\n", bin.path().display()));
}
