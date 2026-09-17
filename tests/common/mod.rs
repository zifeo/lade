//! Shared helpers for integration crates. Each crate compiles this module
//! alone, so helpers unused in one crate would warn. Keep them.
#![allow(dead_code)]

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

pub const SECRET: &str = "tok_example_0000000001";
pub const OTHER: &str = "other_example_0000000002";
pub const LOG_ON: &str = ".:\n  .:\n    log: true\n";

pub fn write_yml(dir: &Path, body: &str) {
    let contents = if body.trim() == "{}" {
        LOG_ON.to_string()
    } else {
        format!("{LOG_ON}{body}")
    };
    fs::write(dir.join("lade.yml"), contents).unwrap();
}

pub fn write_yml_raw(dir: &Path, body: &str) {
    fs::write(dir.join("lade.yml"), body).unwrap();
}

pub fn inject(home: &Path, dir: &Path, args: &[&str]) {
    let _ = lade(home)
        .current_dir(dir)
        .args(std::iter::once("inject").chain(args.iter().copied()))
        .output()
        .unwrap();
}

pub fn log_rows(home: &Path, dir: &Path) -> serde_json::Value {
    let out = lade(home)
        .current_dir(dir)
        .args(["log", "--json", "--since", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

pub fn stored_command(home: &Path, dir: &Path) -> String {
    row_line(&log_rows(home, dir)[0])
}

pub fn row_line(row: &serde_json::Value) -> String {
    let command = row["command"].as_str().unwrap_or("");
    match &row["argv"] {
        serde_json::Value::Array(items) if !items.is_empty() => {
            let rest: Vec<&str> = items.iter().filter_map(|item| item.as_str()).collect();
            if rest.is_empty() {
                command.to_string()
            } else {
                format!("{} {}", command, rest.join(" "))
            }
        }
        serde_json::Value::Object(map) if !map.is_empty() => {
            format!("{command} {}", row["argv"])
        }
        _ => command.to_string(),
    }
}

pub fn filter_log(home: &Path, dir: &Path, extra: &[&str]) -> Vec<serde_json::Value> {
    let mut args = vec!["log", "--json", "--since", "1d"];
    args.extend(extra);
    let out = lade(home)
        .current_dir(dir)
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

pub fn init_git(dir: &Path) {
    fs::create_dir(dir.join(".git")).unwrap();
    fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::create_dir_all(dir.join(".git/refs/heads")).unwrap();
    fs::write(dir.join(".git/refs/heads/main"), "a".repeat(40)).unwrap();
}

pub fn lade_user(home: &Path) -> Command {
    let mut cmd = lade(home);
    cmd.env("USER", "alice");
    cmd
}

pub fn share_pack(home: &Path, dir: &Path, extra: &[&str]) -> PathBuf {
    let mut args = vec!["log", "share"];
    args.extend(extra);
    let out = lade_user(home)
        .current_dir(dir)
        .args(&args)
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&out);
    if let Some(idx) = extra.iter().position(|a| *a == "-o") {
        let path = PathBuf::from(extra[idx + 1]);
        let path = if path.is_absolute() {
            path
        } else {
            dir.join(path)
        };
        assert!(path.is_file(), "expected -o pack at {}", path.display());
        return path;
    }
    let name = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("lade-alice-") && n.ends_with(".tar.gz"))
        })
        .expect("pack in cwd");
    assert!(
        stderr.contains(name.to_str().unwrap()),
        "stderr should mention pack path: {stderr}"
    );
    name
}

pub fn leftover_src_dirs_in(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("lade-log-src-"))
        })
        .count()
}

pub fn backdate_all(home: &Path, ts: &str) {
    let conn = rusqlite::Connection::open(home.join("events.db")).unwrap();
    conn.execute(
        "UPDATE events SET ts = ?1, seq = NULL, prev_hash = NULL, row_hash = NULL, seal_ver = NULL",
        [ts],
    )
    .unwrap();
    let _ = conn.execute("DELETE FROM chain_head", []);
}

pub fn lade_std(home: &Path) -> StdCommand {
    let config_path = home.join("lade-config.json");
    if !config_path.exists() {
        // Far-future stamp: tests must not hit GitHub on `set` / `status`.
        std::fs::write(
            &config_path,
            format!(
                r#"{{"update_check":"2099-01-01T00:00:00Z","self_version":"{}","user":null,"cli_check":{{}}}}"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .unwrap();
    }
    let mut cmd = StdCommand::new(assert_cmd::cargo::cargo_bin("lade"));
    // Drop leftover preexec protocol from the developer shell. An inherited
    // LADE_T would replay a ticket (often the repo `.` catch-all) and skip
    // the temp lade.yml the test just wrote.
    cmd.env("LADE_SHELL", "bash")
        .env("HOME", home)
        .env("LADE_CONFIG_PATH", config_path)
        .env("LADE_EVENTS_PATH", home.join("events.db"))
        .env_remove("LADE_VIA")
        .env_remove("LADE_T")
        .env_remove("LADE_RESTORE")
        .env_remove("LADE_APPROVE")
        .env_remove("AI_AGENT")
        .env_remove("AGENT")
        .env_remove("CLAUDECODE")
        .env_remove("CURSOR_AGENT")
        .env_remove("COPILOT_MODEL")
        .env_remove("CURSOR_VERSION")
        .env_remove("CLAUDE_CODE")
        .env_remove("CURSOR_EXTENSION_HOST_ROLE")
        .env_remove("CURSOR_SANDBOX")
        .env_remove("CODEX_HOME")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SANDBOX")
        .env_remove("CODEX_CI")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_PID");
    cmd
}

pub fn lade(home: &Path) -> Command {
    Command::from_std(lade_std(home))
}

pub fn extract_lade_t(stdout: &str) -> Option<String> {
    for prefix in ["export LADE_T='", "LADE_T='", "export LADE_T="] {
        let Some(start) = stdout.find(prefix) else {
            continue;
        };
        let rest = &stdout[start + prefix.len()..];
        let raw = rest.split([';', '\'', '"', ' ', '\n']).next().unwrap_or("");
        if raw.len() == 4 && raw.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Some(raw.to_string());
        }
    }
    None
}

#[cfg(unix)]
pub mod child;

#[cfg(unix)]
pub fn seed_store_cli(installs: &Path, name: &str, version: &str, src: &Path) {
    let dest_dir = installs.join(name).join(version);
    fs::create_dir_all(&dest_dir).unwrap();
    let dest = dest_dir.join(name);
    let _ = fs::remove_file(&dest);
    std::os::unix::fs::symlink(src, dest).unwrap();
}

#[cfg(unix)]
pub fn fake_cli(dir: &tempfile::TempDir, name: &str, script_body: &str) {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let path = dir.path().join(name);
    fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}
