//! A laptop and a remote on one machine: a fake `ssh` runs the remote
//! command under another HOME and cache, and only the laptop has the
//! provider CLI.
#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;

struct Machines {
    laptop: tempfile::TempDir,
    remote: tempfile::TempDir,
    laptop_bin: tempfile::TempDir,
    remote_bin: tempfile::TempDir,
}

const RULE: &str = "\".\":\n  TOKEN: doppler://api.doppler.com/proj/dev/TOKEN\n";

/// `ssh -N -L <socket>:<host>:<port>` as the laptop hub runs it: forwards
/// each connection to the socket on to host:port.
const FORWARD: &str = r#"import os, socket, sys, threading
path, host, port = sys.argv[1].rsplit(":", 2)
open(os.path.join(os.environ["HOME"], "ssh-tunnel-calls"), "a").write(sys.argv[1] + "\n")
if os.path.exists(path):
    os.unlink(path)
server = socket.socket(socket.AF_UNIX)
server.bind(path)
server.listen(16)
def pipe(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data:
                break
            b.sendall(data)
    except OSError:
        pass
    finally:
        try:
            b.shutdown(socket.SHUT_WR)
        except OSError:
            pass
while True:
    client, _ = server.accept()
    upstream = socket.create_connection((host, int(port)))
    threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()
    threading.Thread(target=pipe, args=(upstream, client), daemon=True).start()
"#;

/// `needle` in the output once its whitespace is collapsed: a message box
/// wraps a long line wherever its width falls.
fn says(needle: &'static str) -> impl predicates::Predicate<str> {
    predicates::function::function(move |text: &str| {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains(needle)
    })
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl Machines {
    fn new() -> Self {
        let machines = Self {
            // Short: the ssh control socket lives under it, and macOS caps
            // socket paths.
            laptop: tempfile::Builder::new()
                .prefix("lade-laptop")
                .tempdir_in("/tmp")
                .unwrap(),
            remote: tempfile::tempdir().unwrap(),
            laptop_bin: tempfile::tempdir().unwrap(),
            remote_bin: tempfile::tempdir().unwrap(),
        };
        let lade = assert_cmd::cargo::cargo_bin("lade");
        std::os::unix::fs::symlink(&lade, machines.remote_bin.path().join("lade")).unwrap();
        executable(
            &machines.laptop_bin.path().join("doppler"),
            "#!/bin/sh\nif [ -e \"$HOME/doppler-logged-out\" ]; then echo 'doppler: you are logged out' >&2; exit 1; fi\necho called >> \"$HOME/doppler-calls\"\necho '{\"TOKEN\":{\"computed\":\"from-the-laptop\"}}'\n",
        );
        let remote_env = machines.remote_env();
        let forward = machines.laptop_bin.path().join("forward.py");
        fs::write(&forward, FORWARD).unwrap();
        executable(
            &machines.laptop_bin.path().join("ssh"),
            &format!(
                "#!/bin/sh\nif [ \"$1\" = -N ]; then\n  for arg in \"$@\"; do [ \"$prev\" = -L ] && spec=$arg; prev=$arg; done\n  exec python3 {forward} \"$spec\"\nfi\nfor last in \"$@\"; do :; done\ncd {home}\nexec env -i {env} /bin/sh -c \"$last\"\n",
                forward = forward.display(),
                home = machines.remote.path().display(),
                env = remote_env
                    .iter()
                    .map(|(k, v)| format!("{k}='{v}'"))
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        );
        let app = machines.remote.path().join("src/app");
        fs::create_dir_all(&app).unwrap();
        common::write_yml_raw(&app, RULE);
        fs::write(
            machines.remote.path().join("lade-config.json"),
            r#"{"user":null}"#,
        )
        .unwrap();
        machines
    }

    fn remote_env(&self) -> Vec<(String, String)> {
        let home = self.remote.path();
        vec![
            ("HOME".into(), home.display().to_string()),
            ("USER".into(), "me".into()),
            (
                "LADE_CACHE_DIR".into(),
                home.join("lade-cache").display().to_string(),
            ),
            (
                "LADE_CONFIG_PATH".into(),
                home.join("lade-config.json").display().to_string(),
            ),
            (
                "LADE_EVENTS_PATH".into(),
                home.join("events.db").display().to_string(),
            ),
            (
                "PATH".into(),
                format!("{}:/usr/bin:/bin", self.remote_bin.path().display()),
            ),
        ]
    }

    fn laptop_path(&self) -> String {
        format!("{}:/usr/bin:/bin", self.laptop_bin.path().display())
    }

    fn on_laptop(&self) -> Command {
        let mut cmd = common::with_lade_env(
            StdCommand::new(assert_cmd::cargo::cargo_bin("lade")),
            self.laptop.path(),
        );
        cmd.env("PATH", self.laptop_path())
            .env_remove("OP_SERVICE_ACCOUNT_TOKEN");
        Command::from_std(cmd)
    }

    fn on_remote(&self, cwd: &Path) -> Command {
        let mut cmd = StdCommand::new(assert_cmd::cargo::cargo_bin("lade"));
        cmd.env_clear().current_dir(cwd);
        for (key, value) in self.remote_env() {
            cmd.env(key, value);
        }
        Command::from_std(cmd)
    }

    fn app(&self) -> PathBuf {
        self.remote.path().join("src/app")
    }

    fn connect(&self) -> assert_cmd::assert::Assert {
        self.connect_path("src/app")
    }

    fn connect_path(&self, path: &str) -> assert_cmd::assert::Assert {
        self.on_laptop()
            .args(["connect", path, "--", "ssh", "build-01"])
            .assert()
    }
}

impl Drop for Machines {
    fn drop(&mut self) {
        let _ = self
            .on_laptop()
            .args(["detach", "--", "ssh", "build-01"])
            .ok();
        let _ = self.on_laptop().args(["ttl", "forget"]).ok();
        let _ = self
            .on_remote(self.remote.path())
            .args(["ttl", "forget"])
            .ok();
    }
}

#[test]
fn connect_resolves_the_walk_on_the_laptop_and_the_remote_gets_its_values() {
    let machines = Machines::new();
    let connected = machines.connect().success();
    let view = String::from_utf8_lossy(&connected.get_output().stderr).into_owned();
    assert!(
        view.contains("doppler://api.doppler.com/proj/dev/TOKEN"),
        "{view}"
    );
    assert!(view.contains("Linked"), "{view}");
    assert!(machines.laptop.path().join("doppler-calls").exists());

    machines
        .on_remote(&machines.app())
        // Lade runs the joined argv as a shell line.
        .args([
            "--",
            "test \"$(echo \"$TOKEN\" | cksum)\" = \"1515322352 16\"",
        ])
        .assert()
        .success();
    assert!(machines.laptop.path().join("doppler-calls").exists());
    // A value the remote typed back into its own command line.
    machines
        .on_remote(&machines.app())
        .args(["ttl", "forget"])
        .assert()
        .success();
    machines
        .on_remote(&machines.app())
        .args(["--", "true from-the-laptop"])
        .assert()
        .success();

    let rows = common::filter_log(
        machines.laptop.path(),
        machines.laptop.path(),
        &["--kind", "remote", "--all"],
    );
    let dump = serde_json::to_string(&rows).unwrap();
    assert!(dump.contains("connect"), "{dump}");
    assert!(dump.contains("served"), "{dump}");
    assert!(
        !dump.contains("from-the-laptop"),
        "diary stored the value: {dump}"
    );
    assert!(dump.matches("\"served\"").count() >= 2, "{dump}");
}

#[test]
fn an_edit_on_the_remote_needs_a_new_connect() {
    let machines = Machines::new();
    machines.connect().success();
    common::write_yml_raw(
        &machines.app(),
        &format!("{RULE}  AGAIN: doppler://api.doppler.com/proj/dev/TOKEN\n"),
    );
    machines
        .on_remote(&machines.app())
        .args(["--", "true"])
        .assert()
        .code(6)
        .stderr(says("lade connect "))
        .stderr(says("src/app -- ssh build-01"));
    machines.connect().success();
    machines
        .on_remote(&machines.app())
        .args(["--", "true"])
        .assert()
        .success();
}

fn doppler_calls(machines: &Machines) -> usize {
    fs::read_to_string(machines.laptop.path().join("doppler-calls"))
        .unwrap_or_default()
        .lines()
        .count()
}

#[test]
fn the_remote_is_served_from_what_connect_cached() {
    let machines = Machines::new();
    machines.connect().success();
    assert_eq!(doppler_calls(&machines), 1);
    machines
        .on_remote(&machines.app())
        .args(["--", "test -n \"$TOKEN\""])
        .assert()
        .success();
    assert_eq!(doppler_calls(&machines), 1);
}

#[test]
fn past_its_ttl_a_value_resolves_again_on_the_laptop_and_a_failure_exits_4() {
    let machines = Machines::new();
    common::write_yml_raw(
        &machines.app(),
        "\".\":\n  \".\":\n    ttl: off\n  TOKEN: doppler://api.doppler.com/proj/dev/TOKEN\n",
    );
    machines.connect().success();
    machines
        .on_remote(&machines.app())
        .args(["--", "test -n \"$TOKEN\""])
        .assert()
        .success();
    assert_eq!(doppler_calls(&machines), 2);
    fs::write(machines.laptop.path().join("doppler-logged-out"), "").unwrap();
    machines
        .on_remote(&machines.app())
        .args(["--", "true"])
        .assert()
        .code(4)
        .stderr(says("you are logged out"))
        .stderr(says("src/app -- ssh build-01"));
}

#[test]
fn a_provider_that_fails_at_connect_fails_connect() {
    let machines = Machines::new();
    fs::write(machines.laptop.path().join("doppler-logged-out"), "").unwrap();
    machines
        .connect()
        .failure()
        .stderr(says("you are logged out"));
}

#[test]
fn a_private_binding_resolves_on_the_laptop_for_a_template_of_the_remote() {
    let machines = Machines::new();
    common::write_yml_raw(
        &machines.app(),
        "\".\":\n  .TOKEN: doppler://api.doppler.com/proj/dev/TOKEN\n  AUTH: \"Bearer ${TOKEN}\"\n",
    );
    machines.connect().success();
    machines
        .on_remote(&machines.app())
        .args([
            "--",
            "test \"${AUTH#Bearer }\" = \"$(echo ZnJvbS10aGUtbGFwdG9w | base64 -d)\" && test -z \"$TOKEN\"",
        ])
        .assert()
        .success();
}

#[test]
fn a_ref_templated_on_a_remote_file_resolves_on_the_laptop_when_the_remote_asks() {
    let machines = Machines::new();
    let name = machines.remote.path().join("name.json");
    fs::write(&name, r#"{"token":"TOKEN"}"#).unwrap();
    common::write_yml_raw(
        &machines.app(),
        &format!(
            "\".\":\n  NAME: file://{}?query=.token\n  DB: doppler://api.doppler.com/proj/dev/${{NAME}}\n",
            name.display()
        ),
    );
    machines
        .connect()
        .success()
        .stderr(says("on first use"))
        .stderr(says("DB needs NAME"));
    assert_eq!(doppler_calls(&machines), 0);
    for _ in 0..2 {
        machines
            .on_remote(&machines.app())
            .args(["--", "test -n \"$DB\""])
            .assert()
            .success();
    }
    assert_eq!(doppler_calls(&machines), 1);
}

#[test]
fn a_ref_templated_on_another_body_resolves_on_the_laptop_when_the_remote_asks() {
    let machines = Machines::new();
    common::write_yml_raw(
        &machines.app(),
        "\".\":\n  NAME: TOKEN\n\"^test\":\n  DB: doppler://api.doppler.com/proj/dev/${NAME}\n",
    );
    machines.connect().success().stderr(says("DB needs NAME"));
    machines
        .on_remote(&machines.app())
        .args(["--", "test \"$(echo \"$DB\" | cksum)\" = \"1515322352 16\""])
        .assert()
        .success();
}

#[test]
fn a_disclaimer_withholds_access_on_the_remote_until_approved_there() {
    let machines = Machines::new();
    common::write_yml_raw(
        &machines.app(),
        "\".\":\n  \".\":\n    disclaimer: Production\n  TOKEN: doppler://api.doppler.com/proj/dev/TOKEN\n",
    );
    machines.connect().success();
    let withheld = machines
        .on_remote(&machines.app())
        .args(["--", "test -n \"$TOKEN\""])
        .assert()
        .code(3)
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&withheld.stderr);
    assert!(stderr.contains("> Production"), "{stderr}");
    let start = stderr.find("LADE_APPROVE=").expect("approval code") + "LADE_APPROVE=".len();
    let code = stderr[start..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect::<String>();
    machines
        .on_remote(&machines.app())
        .env("LADE_APPROVE", &code)
        .args(["--", "test -n \"$TOKEN\""])
        .assert()
        .success();
}

#[test]
fn when_picks_the_body_on_the_remote_by_who_runs_the_command_there() {
    let machines = Machines::new();
    common::write_yml_raw(
        &machines.app(),
        "\".\":\n  - \".\":\n      when: human\n    TOKEN: doppler://api.doppler.com/proj/dev/TOKEN\n  - \".\":\n      when: agent\n    TOKEN: for-agents\n",
    );
    machines.connect().success();
    machines
        .on_remote(&machines.app())
        .args([
            "--",
            "test \"$(echo \"$TOKEN\" | cksum)\" = \"1515322352 16\"",
        ])
        .assert()
        .success();
    machines
        .on_remote(&machines.app())
        .env("CLAUDECODE", "1")
        .args(["--", "test \"$TOKEN\" = for-agents"])
        .assert()
        .success();
}

#[test]
fn the_laptop_resolves_with_the_remote_user_and_a_user_change_needs_a_new_connect() {
    let machines = Machines::new();
    let config = machines.remote.path().join("lade-config.json");
    fs::write(&config, r#"{"user":"alice"}"#).unwrap();
    // The fake doppler knows only TOKEN: resolving bob's entry would fail.
    common::write_yml_raw(
        &machines.app(),
        "\".\":\n  TOKEN:\n    alice: doppler://api.doppler.com/proj/dev/TOKEN\n    bob: doppler://api.doppler.com/proj/dev/OTHER\n",
    );
    machines.connect().success();
    machines
        .on_remote(&machines.app())
        .args(["--", "test -n \"$TOKEN\""])
        .assert()
        .success();
    fs::write(&config, r#"{"user":"bob"}"#).unwrap();
    machines
        .on_remote(&machines.app())
        .args(["--", "true"])
        .assert()
        .code(6)
        .stderr(says("src/app -- ssh build-01"));
}

#[test]
fn a_second_path_shares_the_link() {
    let machines = Machines::new();
    let other = machines.remote.path().join("src/other");
    fs::create_dir_all(&other).unwrap();
    common::write_yml_raw(&other, RULE);
    machines.connect().success();
    machines
        .on_remote(&other)
        .args(["--", "true"])
        .assert()
        .code(6)
        .stderr(says("src/other -- ssh build-01"));
    machines.connect_path("src/other").success();
    for dir in [machines.app(), other] {
        machines
            .on_remote(&dir)
            .args(["--", "test -n \"$TOKEN\""])
            .assert()
            .success();
    }
    let status = machines
        .on_laptop()
        .args(["status", "--json"])
        .output()
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    let links = status["remote"]["links"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0]["walks"].as_array().unwrap().len(), 2, "{links:?}");
}

fn link_phases(machines: &Machines) -> Vec<(String, String)> {
    let status = machines
        .on_laptop()
        .args(["status", "--json"])
        .output()
        .unwrap();
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    status["remote"]["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|link| {
            (
                link["transport"].as_array().unwrap()[1]
                    .as_str()
                    .unwrap()
                    .to_string(),
                link["phase"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn a_second_transport_to_the_machine_replaces_the_link_and_the_old_one_gives_way() {
    let machines = Machines::new();
    machines.connect().success();
    // The fake ssh reaches the same remote whatever the host.
    machines
        .on_laptop()
        .args(["connect", "src/app", "--", "ssh", "build-01-alias"])
        .assert()
        .success();
    let marker = || fs::read_to_string(remote_marker(&machines)).unwrap();
    assert!(marker().contains("build-01-alias"), "{}", marker());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        let phases = link_phases(&machines);
        if phases.contains(&("build-01".to_string(), "failed".to_string())) {
            assert!(
                phases.contains(&("build-01-alias".to_string(), "ready".to_string())),
                "{phases:?}"
            );
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{phases:?}");
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert!(marker().contains("build-01-alias"), "{}", marker());
    machines
        .on_remote(&machines.app())
        .args(["--", "test -n \"$TOKEN\""])
        .assert()
        .success();
    machines
        .on_laptop()
        .args(["detach", "--", "ssh", "build-01-alias"])
        .assert()
        .success();
}

#[test]
fn a_detached_link_fails_closed_and_detach_restores_local_resolution() {
    let machines = Machines::new();
    machines.connect().success();
    machines
        .on_laptop()
        .args(["detach", "--", "ssh", "build-01"])
        .assert()
        .success();
    // The laptop's detach reaches the remote, which leaves attached mode:
    // doppler is then looked up on the remote, which has none.
    let mut attached = true;
    for _ in 0..50 {
        if !machines
            .remote
            .path()
            .join("lade-cache/up/attached.json")
            .exists()
        {
            attached = false;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(!attached, "the remote stayed attached");
    let output = machines
        .on_remote(&machines.app())
        .args(["--", "true"])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert_ne!(output.status.code(), Some(4));
}

#[test]
fn a_link_down_fails_with_exit_4_and_the_connect_line() {
    let machines = Machines::new();
    let up = machines.remote.path().join("lade-cache/up");
    fs::create_dir_all(&up).unwrap();
    fs::write(
        up.join("attached.json"),
        r#"{"laptop":"me@laptop","since":"2026-11-21T00:00:00Z","protocol":1,"transport":"ssh build-01","pid":0}"#,
    )
    .unwrap();
    machines
        .on_remote(&machines.app())
        .args(["--", "true"])
        .assert()
        .code(4)
        .stderr(says("lade connect "))
        .stderr(says("src/app -- ssh build-01"));
}

#[test]
fn other_transports_are_refused() {
    let machines = Machines::new();
    machines
        .on_laptop()
        .args(["connect", "src/app", "--", "kubectl", "exec", "pod"])
        .assert()
        .failure()
        .stderr(says("ssh"));
}

fn attached_marker(machines: &Machines) {
    let up = machines.remote.path().join("lade-cache/up");
    fs::create_dir_all(&up).unwrap();
    fs::write(
        up.join("attached.json"),
        r#"{"laptop":"me@laptop","since":"2026-11-21T00:00:00Z","protocol":1,"transport":"ssh build-01","pid":0}"#,
    )
    .unwrap();
}

#[test]
fn a_tunnel_while_the_link_is_down_fails_with_exit_4() {
    let machines = Machines::new();
    attached_marker(&machines);
    common::write_yml_raw(
        &machines.app(),
        "\".\":\n  DB_PORT: ssh://jump.example.com:22/db.internal/5432\n",
    );
    machines
        .on_remote(&machines.app())
        .args(["--", "true"])
        .assert()
        .code(4)
        .stderr(says("lade attach is not running"))
        .stderr(says("db.internal:5432"));
}

/// A database only the laptop reaches: answers `pong:<line>`.
fn laptop_service() -> u16 {
    use std::io::{BufRead, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut line = String::new();
                let mut reader = std::io::BufReader::new(&stream);
                if reader.read_line(&mut line).is_ok() {
                    let _ = (&stream).write_all(format!("pong:{line}").as_bytes());
                }
            });
        }
    });
    port
}

const ASK_SERVICE: &str = r#"python3 -c 'import os, socket; s = socket.create_connection(("127.0.0.1", int(os.environ["ECHO_PORT"]))); s.sendall(b"ping\n"); print(s.recv(100).decode())'"#;

#[test]
fn a_remote_tunnel_runs_on_the_laptop_and_carries_connections() {
    let port = laptop_service();
    let machines = Machines::new();
    let uri = format!("ssh://jump.example.com/127.0.0.1/{port}");
    common::write_yml_raw(&machines.app(), &format!("{RULE}  ECHO_PORT: {uri}\n"));
    machines
        .connect()
        .success()
        .stderr(predicates::str::contains(uri.as_str()));
    for _ in 0..2 {
        machines
            .on_remote(&machines.app())
            .args(["--", ASK_SERVICE])
            .assert()
            .success()
            .stdout(predicates::str::contains("pong:ping"));
    }
    let calls = fs::read_to_string(machines.laptop.path().join("ssh-tunnel-calls")).unwrap();
    assert!(calls.contains(&format!("127.0.0.1:{port}")), "{calls}");
    let rows = common::filter_log(
        machines.laptop.path(),
        machines.laptop.path(),
        &["--kind", "remote", "--all"],
    );
    let dump = serde_json::to_string(&rows).unwrap();
    assert!(dump.contains("\"tunnel\""), "{dump}");
}

/// Throwaway: generated for this test only.
const AGE_KEY: &str = "AGE-SECRET-KEY-1NE37NHXJLTV89NPRXUG5MN9A93UGJV00JRKH6H38G3A2XRN4PHCSEURA5G";
const AGE_RECIPIENT: &str = "age1uvdq3fcl0tmld47pj8mhhuuth0tf5qjgyg2azuhztsjxltz78c6q028fat";

/// A Vault only the laptop reaches, over plain HTTP.
fn laptop_vault() -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 8192];
            let _ = stream.read(&mut request);
            let body = r#"{"data":{"data":{"password":"from-the-laptop"}}}"#;
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });
    port
}

#[test]
fn every_secret_provider_resolves_through_the_link() {
    let machines = Machines::new();
    let bin = machines.laptop_bin.path();
    for (name, body) in [
        (
            "infisical",
            r#"echo '[{"key":"TOKEN","value":"from-the-laptop"}]'"#,
        ),
        ("op", "cat >/dev/null; printf '%s' from-the-laptop"),
        ("passbolt", r#"echo '{"password":"from-the-laptop"}'"#),
        ("aws", r#"echo '{"SecretString":"from-the-laptop"}'"#),
        ("az", r#"echo '{"value":"from-the-laptop"}'"#),
        (
            "bw",
            r#"echo '[{"id":"myitem","name":"myitem","login":{"password":"from-the-laptop"}}]'"#,
        ),
        ("gcloud", "printf '%s' from-the-laptop"),
    ] {
        executable(&bin.join(name), &format!("#!/bin/sh\n{body}\n"));
    }
    let sealed = lade_sdk::upstream::seal(AGE_RECIPIENT, b"from-the-laptop").unwrap();
    let age = sealed
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect::<String>();
    let refs = [
        (
            "DOPPLER",
            "doppler://api.doppler.com/proj/dev/TOKEN".to_string(),
        ),
        (
            "INFISICAL",
            "infisical://app.infisical.com/proj/dev/TOKEN".to_string(),
        ),
        ("OP", "op://my.1password.com/vault/item/field".to_string()),
        (
            "VAULT",
            format!("vault://127.0.0.1:{}/secret/app/password", laptop_vault()),
        ),
        (
            "PASSBOLT",
            "passbolt://passbolt.example.com/resource-uuid/password".to_string(),
        ),
        ("AWSSM", "awssm://us-east-1/app/token".to_string()),
        ("AZUREKV", "azurekv://myvault/token".to_string()),
        ("BW", "bw://myitem/password".to_string()),
        ("GCPSM", "gcpsm://myproj/token".to_string()),
        ("AGE", format!("age://{age}")),
    ];
    let yml = refs
        .iter()
        .map(|(name, uri)| format!("  {name}: {uri}\n"))
        .collect::<String>();
    common::write_yml_raw(&machines.app(), &format!("\".\":\n{yml}"));
    let mut connect = machines.on_laptop();
    connect
        .env("VAULT_TOKEN", "test")
        .env("LADE_VAULT_HTTP", "1")
        .env("LADE_AGE_KEY", AGE_KEY)
        .args(["connect", "src/app", "--", "ssh", "build-01"])
        .assert()
        .success();
    // Each value is `from-the-laptop`, which Lade scrubs from the line itself.
    let check = refs
        .iter()
        .map(|(name, _)| {
            format!("test \"$(echo \"${name}\" | cksum)\" = \"1515322352 16\" || {{ echo {name} is wrong; exit 1; }}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    machines
        .on_remote(&machines.app())
        .args(["--", &check])
        .assert()
        .success();
}

#[test]
fn a_tunnel_open_for_one_path_is_not_reused_by_a_path_no_connect_resolved() {
    let port = laptop_service();
    let machines = Machines::new();
    let rule = format!("\".\":\n  ECHO_PORT: ssh://jump.example.com/127.0.0.1/{port}\n");
    common::write_yml_raw(&machines.app(), &rule);
    let other = machines.remote.path().join("src/other");
    fs::create_dir_all(&other).unwrap();
    common::write_yml_raw(&other, &rule);
    machines.connect().success();

    // Holds the tunnel open on the remote hub while `src/other` asks for it.
    let mut holder = StdCommand::new(assert_cmd::cargo::cargo_bin("lade"));
    holder.env_clear().current_dir(machines.app());
    for (key, value) in machines.remote_env() {
        holder.env(key, value);
    }
    let mut holder = holder.args(["--", "sleep 5"]).spawn().unwrap();
    let calls = machines.laptop.path().join("ssh-tunnel-calls");
    for _ in 0..100 {
        if calls.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(calls.exists(), "the holder's tunnel never started");

    machines
        .on_remote(&other)
        .args(["--", ASK_SERVICE])
        .assert()
        .code(6);
    let _ = holder.kill();
    let _ = holder.wait();
}

fn remote_marker(machines: &Machines) -> PathBuf {
    machines.remote.path().join("lade-cache/up/attached.json")
}

#[test]
fn a_detach_on_the_remote_while_the_link_is_down_is_not_undone_by_the_laptop() {
    let machines = Machines::new();
    machines.connect().success();
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(remote_marker(&machines)).unwrap()).unwrap();
    let pid = marker["pid"].as_i64().unwrap() as i32;
    // The transport drops: the laptop hub retries one second later.
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    machines
        .on_remote(machines.remote.path())
        .arg("detach")
        .assert()
        .success();
    let mut refused = false;
    for _ in 0..100 {
        let status = machines
            .on_laptop()
            .args(["status", "--json"])
            .output()
            .unwrap();
        if String::from_utf8_lossy(&status.stdout).contains("was detached with lade detach") {
            refused = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(refused, "the laptop hub did not give up its link");
    assert!(
        !remote_marker(&machines).exists(),
        "the remote was attached again"
    );
}

#[test]
fn a_detach_on_the_laptop_without_a_link_ends_attached_mode_on_the_remote() {
    let machines = Machines::new();
    attached_marker(&machines);
    machines
        .on_laptop()
        .args(["detach", "--", "ssh", "build-01"])
        .assert()
        .success()
        .stderr(says("lade detach ran there"));
    assert!(!remote_marker(&machines).exists());
}

#[test]
fn a_laptop_tunnel_that_fails_fails_the_remote_command_with_its_error() {
    let machines = Machines::new();
    common::write_yml_raw(
        &machines.app(),
        &format!("{RULE}  DB_PORT: ssh://jump.example.com/db.internal/5432\n"),
    );
    machines.connect().success();
    let ssh = machines.laptop_bin.path().join("ssh");
    let script = fs::read_to_string(&ssh).unwrap();
    let start = script.find("  exec python3").unwrap();
    let end = start + script[start..].find('\n').unwrap();
    let failing = format!(
        "{}  echo 'jump.example.com: Connection refused' >&2; exit 255{}",
        &script[..start],
        &script[end..]
    );
    executable(&ssh, &failing);
    // The laptop retries its tunnel for its own 60 s start budget, and the
    // remote hub's longer budget ends with that error.
    let started = std::time::Instant::now();
    machines
        .on_remote(&machines.app())
        .args(["--", "true"])
        .assert()
        .failure()
        .stderr(says("Connection refused"));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(120),
        "took {:?}",
        started.elapsed()
    );
}
