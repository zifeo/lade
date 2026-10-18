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
    // `env -i` drops HOME and the shells fall back to the real one. The
    // vault bodies here are cacheable, so keep the hub out of the user's
    // cache dir and keychain. A temp HOME would also cold-start mise and
    // the `cargo run` debug hook.
    let cache = tempdir().expect("lade cache");
    let env = [
        "-i".to_string(),
        format!("PATH={}", path_env()),
        format!("LADE_CACHE_DIR={}", cache.path().display()),
        format!("LADE_WRAP_KEY={}", "ab".repeat(32)),
        format!("MISE_INSTALLS_DIR={}", installs.path().display()),
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
