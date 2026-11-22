mod common;

use std::fs;
use std::path::Path;

use predicates::prelude::PredicateBooleanExt;
use tempfile::tempdir;

fn lade_process(home: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("lade"));
    let config_path = home.join("lade-config.json");
    if !config_path.exists() {
        fs::write(&config_path, r#"{"user":null}"#).unwrap();
    }
    cmd.env("HOME", home)
        .env("LADE_CONFIG_PATH", config_path)
        .env_remove("LADE_VIA")
        .env_remove("AI_AGENT")
        .env_remove("AGENT")
        .env_remove("CLAUDECODE")
        .env_remove("CURSOR_AGENT")
        .env_remove("COPILOT_MODEL")
        .env_remove("CURSOR_VERSION")
        .env_remove("CLAUDE_CODE")
        .env_remove("CURSOR_EXTENSION_HOST_ROLE")
        .env_remove("CURSOR_SANDBOX")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SANDBOX")
        .env_remove("CODEX_CI")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_PID");
    cmd
}

#[test]
fn test_mcp_stdio_injects_public_bindings_only() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^env$\":\n  .TOKEN: hidden\n  PUBLIC: \"Bearer ${TOKEN}\"\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["mcp", "--", "env"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PUBLIC=Bearer hidden"))
        .stdout(predicates::str::contains("TOKEN=hidden").not());
}

#[test]
fn test_mcp_agent_when_uses_env_signal() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^env$\":\n  \".\":\n    when: agent\n  PUBLIC: agentsecret\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["mcp", "--", "env"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PUBLIC=agentsecret"));
    common::lade(home.path())
        .current_dir(dir.path())
        .env("CURSOR_AGENT", "1")
        .args(["mcp", "--", "env"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PUBLIC=agentsecret"));
}

#[cfg(unix)]
#[test]
fn test_mcp_restarts_stdio_child_before_initialize_without_rehydrating() {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let hydrates = dir.path().join("hydrates");
    let started = dir.path().join("started");
    let server = dir.path().join("crash-once");
    fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"^crash-once$\":\n  TOKEN: \"sh://printf x >> {}; printf cached-secret\"\n",
            hydrates.display()
        ),
    )
    .unwrap();
    fs::write(
        &server,
        format!(
            "#!/bin/sh\nif [ ! -f '{started}' ]; then touch '{started}'; exit 1; fi\nprintf '%s\\n' \"$TOKEN\"\nexec cat\n",
            started = started.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&server, fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = lade_process(home.path())
        .current_dir(dir.path())
        .args(["mcp", "--", "crash-once"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string())
            ),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().expect("piped stdout");
    let stdin = child.stdin.take().expect("piped stdin");
    let mut output = Vec::new();
    let started_at = Instant::now();
    while !output
        .windows(b"cached-secret".len())
        .any(|w| w == b"cached-secret")
    {
        assert!(
            started_at.elapsed() < Duration::from_secs(8),
            "lade mcp dropped the child instead of restarting: {}",
            String::from_utf8_lossy(&output)
        );
        let mut buf = [0; 64];
        let read = stdout.read(&mut buf).unwrap();
        assert_ne!(
            read,
            0,
            "stdout closed before restart: {}",
            String::from_utf8_lossy(&output)
        );
        output.extend_from_slice(&buf[..read]);
    }
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success(), "{status}");
    assert_eq!(fs::read_to_string(&hydrates).unwrap(), "x");
}

#[test]
fn mcp_withheld_disclaimer_writes_denied() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^env$\":\n  .:\n    log: true\n    disclaimer: \"Danger!\"\n  PUBLIC: val\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["mcp", "--", "env"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("PUBLIC=val"),
        "a withheld disclaimer must not start the server with its access"
    );
    let rows = common::log_rows(home.path(), dir.path());
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["kind"], "denied");
}

#[test]
fn a_failed_secret_stops_the_server_before_it_starts() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let missing = dir.path().join("missing.json");
    fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"^sh\":\n  KEY: \"file://{}?query=.token\"\n",
            missing.display()
        ),
    )
    .unwrap();
    let started = dir.path().join("started");
    common::lade(home.path())
        .current_dir(dir.path())
        .args([
            "mcp",
            "--",
            "sh",
            "-c",
            &format!("touch '{}'", started.display()),
        ])
        .assert()
        .code(1);
    assert!(!started.exists());
}

#[test]
fn test_mcp_requires_one_transport_target() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .arg("mcp")
        .assert()
        .failure()
        .stderr(predicates::str::contains("provide an MCP HTTPS URL"));
}

#[cfg(unix)]
#[test]
fn a_server_a_signal_ends_exits_lade_mcp_with_128_plus_the_signal() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let server = dir.path().join("term-self");
    fs::write(&server, "#!/bin/sh\nread line\nkill -TERM $$\n").unwrap();
    fs::set_permissions(&server, fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = lade_process(home.path())
        .current_dir(dir.path())
        .args(["mcp", "--", server.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().expect("piped stdin");
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    drop(stdin);
    assert_eq!(
        output.status.code(),
        Some(143),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn a_client_that_leaves_while_the_server_restarts_gets_the_servers_code() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let server = dir.path().join("exit-seven");
    fs::write(&server, "#!/bin/sh\nexit 7\n").unwrap();
    fs::set_permissions(&server, fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = lade_process(home.path())
        .current_dir(dir.path())
        .args(["mcp", "--", server.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().expect("piped stdin");
    // The server exits at once and Lade waits out a restart backoff.
    std::thread::sleep(std::time::Duration::from_millis(600));
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
