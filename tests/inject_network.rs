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

/// `lade` with `bin` first on PATH and a hub socket path short enough for
/// macOS.
#[cfg(unix)]
fn lade_with_bin(home: &std::path::Path, bin: &std::path::Path) -> std::process::Command {
    let mut cmd = common::lade_std(home);
    cmd.env(
        "PATH",
        format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
    )
    .env("LADE_CACHE_DIR", home.join("c"))
    .stdin(std::process::Stdio::null());
    cmd
}

#[cfg(unix)]
#[test]
fn test_inject_network_exports_the_picked_port() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let bin = tempdir().unwrap();
    common::write_fake_ssh(bin.path());
    common::child::write_executable(
        &dir.path().join("check.sh"),
        "#!/bin/sh\necho \"$DB_PORT\"\nif /usr/bin/python3 -c 'import socket,sys; socket.socket().bind((\"127.0.0.1\", int(sys.argv[1])))' \"$DB_PORT\" 2>/dev/null; then echo free; else echo held; fi\n",
    );
    fs::write(
        dir.path().join("lade.yml"),
        "\"^sh check\":\n  DB_PORT: ssh://jump.example.com/db.internal/5432\n",
    )
    .unwrap();
    let out = lade_with_bin(home.path(), bin.path())
        .current_dir(dir.path())
        .env("FAKE_SSH_PID", dir.path().join("ssh.pid"))
        .args(["inject", "sh check.sh"])
        .output()
        .unwrap();
    let _ = lade_with_bin(home.path(), bin.path())
        .current_dir(dir.path())
        .args(["log", "prune", "--hub"])
        .output();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<&str> = stdout.lines().collect();
    let port: u16 = lines[0].parse().unwrap_or_else(|_| panic!("{stdout}"));
    assert!(port > 0);
    assert_eq!(lines[1], "held", "{stdout}");
}

#[cfg(unix)]
#[test]
fn test_a_failed_tunnel_stops_the_others() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let bin = tempdir().unwrap();
    let pid_file = dir.path().join("ssh.pid");
    // An ssh that starts and never listens: its tunnel stays starting.
    common::child::write_executable(
        &bin.path().join("ssh"),
        "#!/bin/sh\necho $$ > \"$FAKE_SSH_PID\"\nexec sleep 60\n",
    );
    let holder = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let held = holder.local_addr().unwrap().port();
    fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"^true\":\n  DB_PORT: ssh://jump.example.com/slow.internal/5432\n  {held}: ssh://jump.example.com/db.internal/5432\n"
        ),
    )
    .unwrap();
    let out = lade_with_bin(home.path(), bin.path())
        .current_dir(dir.path())
        .env("FAKE_SSH_PID", &pid_file)
        .args(["inject", "true"])
        .output()
        .unwrap();
    // The held port fails at once, so the other start may not have spawned
    // its ssh yet. When it did, that ssh must go with the command.
    let provider = pid_file
        .exists()
        .then(|| common::child::read_pid(&pid_file));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let alive = provider.is_some_and(|pid| {
        while common::child::pid_alive(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let alive = common::child::pid_alive(pid);
        if alive {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        alive
    });
    let _ = lade_with_bin(home.path(), bin.path())
        .current_dir(dir.path())
        .args(["log", "prune", "--hub"])
        .output();

    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains(&format!("Local port {held} is already in use")),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!alive, "the starting ssh outlived the failed command");
}

#[cfg(unix)]
#[test]
fn test_an_interrupt_while_tunnels_start_leaves_no_file_output() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let bin = tempdir().unwrap();
    let pid_file = dir.path().join("ssh.pid");
    // An ssh that starts and never listens: its tunnel stays starting.
    common::child::write_executable(
        &bin.path().join("ssh"),
        "#!/bin/sh\necho $$ > \"$FAKE_SSH_PID\"\nexec sleep 60\n",
    );
    fs::write(
        dir.path().join("lade.yml"),
        "\"^true\":\n  DB_PORT: ssh://jump.example.com/slow.internal/5432\n\"^true \":\n  .:\n    file: out.json\n  KEY: value\n",
    )
    .unwrap();
    let mut lade = lade_with_bin(home.path(), bin.path())
        .current_dir(dir.path())
        .env("FAKE_SSH_PID", &pid_file)
        .args(["inject", "true now"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !pid_file.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // The secrets are hydrated well before the tunnel would answer.
    std::thread::sleep(std::time::Duration::from_millis(300));
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(lade.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let status = lade.wait().unwrap();
    if pid_file.exists() {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(common::child::read_pid(&pid_file) as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    let _ = lade_with_bin(home.path(), bin.path())
        .current_dir(dir.path())
        .args(["log", "prune", "--hub"])
        .output();

    assert!(pid_file.exists(), "the tunnel never started");
    assert_eq!(status.code(), Some(130), "{status:?}");
    assert!(!dir.path().join("out.json").exists());
}
