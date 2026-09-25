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

pub fn lock_tool_version(name: &str) -> String {
    let body = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("lade.lock"))
        .expect("lade.lock");
    let headers = [format!("[[tools.{name}]]"), format!("[[tools.\"{name}\"]]")];
    for header in headers {
        if let Some(rest) = body.split(&header).nth(1)
            && let Some(version) = version_in_block(rest)
        {
            return version;
        }
    }
    for chunk in body.split("[[tools.") {
        if chunk.contains(name)
            && let Some(version) = version_in_block(chunk)
        {
            return version;
        }
    }
    panic!("{name} missing from lade.lock");
}

fn version_in_block(rest: &str) -> Option<String> {
    rest.lines().find_map(|line| {
        line.trim()
            .strip_prefix("version = \"")
            .and_then(|value| value.strip_suffix('"'))
            .map(str::to_string)
    })
}

pub fn seed_stub_cli(installs: &Path, name: &str, version: &str) {
    seed_stub_bin(installs, name, version, name);
}

pub fn seed_stub_bin(installs: &Path, tool: &str, version: &str, bin: &str) {
    let dest_dir = installs.join(tool).join(version);
    std::fs::create_dir_all(&dest_dir).unwrap();
    let dest = dest_dir.join(bin);
    std::fs::write(&dest, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// kubectl on `.` still wraps every command. Shell pins do not.
pub fn seed_workspace_pins(installs: &Path) {
    let pins = [
        ("op", "1password/cli"),
        ("fish", "fish-shell"),
        ("zsh", "zsh-bin"),
        ("kubectl", "kubectl"),
        ("k3d", "k3d"),
        ("vault", "vault"),
    ];
    for (bin, lock_name) in pins {
        seed_stub_cli(installs, bin, &lock_tool_version(lock_name));
    }
    let rust = lock_tool_version("rust");
    seed_stub_bin(installs, "rust", &rust, "cargo");
    seed_stub_bin(installs, "rust", &rust, "rustc");
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
    let config_path = home.join("lade-config.json");
    if !config_path.exists() {
        std::fs::write(
            &config_path,
            format!(
                r#"{{"update_check":"2099-01-01T00:00:00Z","self_version":"{}","user":null,"cli_check":{{}}}}"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .unwrap();
    }
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("lade"));
    cmd.env("LADE_SHELL", "bash")
        .env("HOME", home)
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
