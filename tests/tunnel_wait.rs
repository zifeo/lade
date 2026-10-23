#![cfg(unix)]

//! Two commands on one fixed port, through a real hub: a second target waits
//! for the first command to end, and an idle tunnel gives its port up at once.

mod common;

use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::child::{pid_alive, read_pid};
use tempfile::{TempDir, tempdir};

struct Setup {
    home: TempDir,
    bin: TempDir,
    port: u16,
}

impl Setup {
    fn new() -> Self {
        let bin = tempdir().unwrap();
        common::write_fake_ssh(bin.path());
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        Self {
            home: tempdir().unwrap(),
            bin,
            port,
        }
    }

    /// A project whose `sleep` rule forwards the fixed port to `target`.
    fn project(&self, target: &str, ttl: bool) -> TempDir {
        let dir = tempdir().unwrap();
        let body = if ttl { "  .:\n    ttl: 1h\n" } else { "" };
        common::write_yml_raw(
            dir.path(),
            &format!(
                "\"sleep.*\":\n{body}  {}: ssh://jump.example.com/{target}/5432\n",
                self.port
            ),
        );
        dir
    }

    fn inject(&self, project: &Path, command: &str) -> Command {
        let mut cmd = common::lade_std(self.home.path());
        cmd.current_dir(project)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.bin.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            // The hub socket path must stay under the 104-byte limit.
            .env("LADE_CACHE_DIR", self.home.path().join("c"))
            .env("FAKE_SSH_PID", project.join("ssh.pid"))
            .args(["inject", command])
            .stdin(Stdio::null());
        cmd
    }
}

fn exits_within(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn a_second_target_waits_for_the_first_command_to_end() {
    let setup = Setup::new();
    let prd = setup.project("prd-db", false);
    let stg = setup.project("stg-db", false);
    let mut first = setup
        .inject(prd.path(), "sleep 3")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let prd_provider = read_pid(&prd.path().join("ssh.pid"));

    let waited = Instant::now();
    let second = setup.inject(stg.path(), "sleep 0").output().unwrap();

    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(second.status.success(), "{stderr}");
    assert!(waited.elapsed() >= Duration::from_secs(1), "did not wait");
    assert!(first.wait().unwrap().success());
    assert!(exits_within(prd_provider, Duration::from_secs(5)));
    read_pid(&stg.path().join("ssh.pid"));
}

#[test]
fn an_idle_tunnel_gives_its_port_to_another_target() {
    let setup = Setup::new();
    let prd = setup.project("prd-db", true);
    let stg = setup.project("stg-db", false);
    let first = setup.inject(prd.path(), "sleep 0").output().unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let prd_provider = read_pid(&prd.path().join("ssh.pid"));
    assert!(pid_alive(prd_provider), "the ttl keeps it");

    let started = Instant::now();
    let second = setup.inject(stg.path(), "sleep 0").output().unwrap();

    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(second.status.success(), "{stderr}");
    assert!(started.elapsed() < Duration::from_secs(10), "it waited");
    assert!(exits_within(prd_provider, Duration::from_secs(5)));
}
