use assert_cmd::Command;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

pub fn seed_store_cli(installs: &Path, name: &str, version: &str, src: &Path) {
    std::fs::create_dir_all(installs.join(name).join(version)).unwrap();
    let dest = installs.join(name).join(version).join(name);
    let _ = std::fs::remove_file(&dest);
    #[cfg(unix)]
    std::os::unix::fs::symlink(src, dest).unwrap();
}

/// A pinned CLI that only has to exist: `name` at the version `lade.lock`
/// locks for `lock_name`.
pub fn seed_locked_stub(installs: &Path, name: &str, lock_name: &str) {
    let lock = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("lade.lock"))
        .expect("lade.lock");
    let version = lock
        .split("[[tools.")
        .find(|block| {
            block
                .lines()
                .next()
                .is_some_and(|header| header.contains(lock_name))
        })
        .and_then(|block| {
            block.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("version = \"")
                    .and_then(|value| value.strip_suffix('"'))
            })
        })
        .unwrap_or_else(|| panic!("{lock_name} missing from lade.lock"));
    let dir = installs.join(name).join(version);
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join(name);
    std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

pub fn command_path(name: &str) -> Option<PathBuf> {
    let output = StdCommand::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

pub fn lade(home: &std::path::Path) -> Command {
    Command::from_std(lade_std(home))
}

pub fn lade_std(home: &std::path::Path) -> StdCommand {
    let config_path = home.join("lade-config.json");
    if !config_path.exists() {
        std::fs::write(&config_path, r#"{"user":null}"#).unwrap();
    }
    // Lade runs a line in the shell that started it; `&& true` keeps bash
    // from replacing itself with Lade.
    let mut cmd = StdCommand::new("/bin/bash");
    cmd.args(["-c", "\"$0\" \"$@\" && true"])
        .arg(assert_cmd::cargo::cargo_bin("lade"));
    cmd.env("HOME", home)
        .env("LADE_CONFIG_PATH", config_path)
        .env_remove("LADE_VIA")
        .env_remove("AI_AGENT")
        .env_remove("AGENT")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CURSOR_AGENT")
        .env_remove("COPILOT_MODEL")
        .env_remove("CURSOR_VERSION")
        .env_remove("CURSOR_EXTENSION_HOST_ROLE")
        .env_remove("CURSOR_SANDBOX")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SANDBOX")
        .env_remove("CODEX_CI")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_PID");
    cmd
}
