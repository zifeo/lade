use crate::common::seed_workspace_pins;
use std::process::{Command, Stdio};
use tempfile::tempdir;

fn repo_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn has_cmd(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd} >/dev/null 2>&1")])
        .status()
        .is_ok_and(|s| s.success())
}

fn require_cmds(cmds: &[&str]) {
    let missing = cmds
        .iter()
        .copied()
        .filter(|cmd| !has_cmd(cmd))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "missing required commands: {}",
        missing.join(", ")
    );
}

fn docker_ready() -> bool {
    Command::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn path_env() -> String {
    std::env::var("PATH").unwrap_or_default()
}

/// Real shell bin dirs first, so a mise shim never shadows a test shell:
/// a shim symlinks to the mise binary, which can refuse to run under the
/// isolated HOME.
fn shell_first_path() -> String {
    let path = path_env();
    let mut entries: Vec<std::path::PathBuf> = Vec::new();
    for shell in ["bash", "zsh", "fish"] {
        let real = std::env::split_paths(&path).find(|dir| {
            let bin = dir.join(shell);
            let shim = std::fs::read_link(&bin)
                .is_ok_and(|target| target.file_name() == Some(std::ffi::OsStr::new("mise")));
            bin.is_file() && !shim
        });
        if let Some(dir) = real
            && !entries.contains(&dir)
        {
            entries.push(dir);
        }
    }
    entries.extend(std::env::split_paths(&path));
    std::env::join_paths(entries)
        .ok()
        .and_then(|joined| joined.into_string().ok())
        .unwrap_or(path)
}

fn run_cmd(cmd: &str, args: &[&str]) {
    let output = Command::new(cmd)
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("spawn command");
    if output.status.success() {
        return;
    }
    panic!(
        "{cmd} {:?} failed:\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn vault_shell_scripts_run_from_cargo_test_workspace() {
    require_cmds(&["bash", "zsh", "fish", "curl", "docker"]);
    assert!(docker_ready(), "docker daemon is required");

    let installs = tempdir().expect("mise installs");
    seed_workspace_pins(installs.path());
    // A temp HOME keeps every Lade and mise write (diary, tickets, config,
    // mise shims) off the developer's machine: with the real one, mise
    // rewrote ~/.local/share/mise/shims from these seeded installs. The
    // scripts call the built binary, so cargo needs no HOME. The hub socket
    // path must stay under the 104-byte limit, and the wrap key keeps the
    // keychain out.
    let home = tempdir().expect("home");
    let config = home.path().join("lade-config.json");
    std::fs::write(&config, r#"{"user":null}"#).expect("write lade config");
    let cache = tempdir().expect("lade cache");
    let mise_cache =
        std::env::var("MISE_CACHE_DIR").expect("MISE_CACHE_DIR from .cargo/config.toml");
    // `docker compose` is a plugin in the developer's docker config.
    let docker_config = std::env::var("DOCKER_CONFIG")
        .unwrap_or_else(|_| format!("{}/.docker", std::env::var("HOME").expect("developer HOME")));
    let env = [
        "-i".to_string(),
        format!("PATH={}", shell_first_path()),
        format!("HOME={}", home.path().display()),
        format!("LADE_CONFIG_PATH={}", config.display()),
        format!("LADE_CACHE_DIR={}", cache.path().display()),
        format!("LADE_WRAP_KEY={}", "ab".repeat(32)),
        format!("MISE_INSTALLS_DIR={}", installs.path().display()),
        format!("MISE_CACHE_DIR={mise_cache}"),
        format!("DOCKER_CONFIG={docker_config}"),
        "VAULT_TOKEN=token".to_string(),
        "LADE_VAULT_HTTP=1".to_string(),
    ];
    for (shell, script) in [
        ("bash", "tests/test_vault.bash"),
        ("zsh", "tests/test_vault.zsh"),
        ("fish", "tests/test_vault.fish"),
    ] {
        let mut args: Vec<&str> = env.iter().map(String::as_str).collect();
        args.extend([shell, script]);
        run_cmd("env", &args);
    }
}
