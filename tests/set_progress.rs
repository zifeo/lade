mod common;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_set_overlay_shows_overridden_progress() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        ".:\n  TOKEN: a\n\"echo\":\n  TOKEN: b\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "echo hi"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export TOKEN='b'"))
        .stderr(predicates::str::contains("TOKEN (overridden)").not());
}

#[test]
fn test_set_git_cancel_shows_cancelled_progress() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        ".:\n  SSH_AUTH_SOCK: \"\"\n\"^git \":\n  SSH_AUTH_SOCK: ~\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "git status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export SSH_AUTH_SOCK").not())
        .stderr(predicates::str::contains("SSH_AUTH_SOCK (cancelled)").not());
}

#[test]
fn test_set_silence_hides_hydration_progress() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"echo\":\n  \".\":\n    silence: true\n  KEY: val\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "echo hi"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export KEY="))
        .stderr(predicates::str::contains("KEY").not());
}

#[test]
fn test_set_without_silence_shows_hydration_progress() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(dir.path().join("lade.yml"), "\"echo\":\n  KEY: val\n").unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "echo hi"])
        .assert()
        .success()
        .stderr(predicates::str::contains("Raw: KEY").not());
}

#[test]
#[cfg(unix)]
fn test_set_with_vault_http() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        let _ = stream.read(&mut buf);
        let body = r#"{"data":{"data":{"password":"vault_injected"}}}"#;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
    });
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = tempdir().unwrap();
    let dest_dir = installs.path().join("vault/1.17.6");
    fs::create_dir_all(&dest_dir).unwrap();
    let dest = dest_dir.join("vault");
    fs::write(&dest, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        format!("\"vault.*\":\n  PASSWORD: \"vault://{host}/secret/myapp/password\"\n"),
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", installs.path())
        .env("VAULT_TOKEN", "s.token")
        .env("LADE_VAULT_HTTP", "1")
        .args(["set", "vault cmd"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "export PASSWORD='vault_injected'",
        ));
    server.join().unwrap();
}

#[test]
fn test_set_skips_agent_when_rules() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  \".\":\n    when: agent\n  SECRET: agentsecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "mycmd"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export SECRET").not());
}

#[test]
fn test_set_fish_still_evals_in_the_interactive_shell() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"mycmd\":\n  SECRET: mysecret\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_SHELL", "fish")
        .args(["set", "mycmd"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(
        stdout.contains("set --global --export SECRET 'mysecret'"),
        "preexec must keep fish set() syntax: {stdout}"
    );
    assert!(
        !stdout.contains("--no-config"),
        "preexec must not switch the interactive shell to --no-config: {stdout}"
    );
}
