use std::process::{Command, Stdio};

fn repo_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn has_cmd(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd} >/dev/null 2>&1")])
        .status()
        .is_ok_and(|s| s.success())
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
fn shell_scripts_run_from_cargo_test_workspace() {
    if !(has_cmd("bash") && has_cmd("zsh") && has_cmd("fish")) {
        eprintln!("skip - bash/zsh/fish are required");
        return;
    }

    let path = path_env();
    run_cmd(
        "env",
        &[
            "-i",
            &format!("PATH={path}"),
            "TEST=ok",
            "bash",
            "scripts/test.bash",
        ],
    );
    run_cmd(
        "env",
        &[
            "-i",
            &format!("PATH={path}"),
            "TEST=ok",
            "zsh",
            "scripts/test.zsh",
        ],
    );
    run_cmd(
        "env",
        &[
            "-i",
            &format!("PATH={path}"),
            "TEST=ok",
            "fish",
            "scripts/test.fish",
        ],
    );
}

#[test]
fn vault_shell_scripts_run_from_cargo_test_workspace() {
    if !(has_cmd("bash") && has_cmd("zsh") && has_cmd("fish") && has_cmd("vault")) {
        eprintln!("skip - bash/zsh/fish/vault are required");
        return;
    }
    if !(has_cmd("docker") && docker_ready()) {
        eprintln!("skip - docker daemon unavailable");
        return;
    }

    let path = path_env();
    run_cmd(
        "env",
        &[
            "-i",
            &format!("PATH={path}"),
            "VAULT_TOKEN=token",
            "LADE_VAULT_HTTP=1",
            "bash",
            "tests/test_vault.bash",
        ],
    );
    run_cmd(
        "env",
        &[
            "-i",
            &format!("PATH={path}"),
            "VAULT_TOKEN=token",
            "LADE_VAULT_HTTP=1",
            "zsh",
            "tests/test_vault.zsh",
        ],
    );
    run_cmd(
        "env",
        &[
            "-i",
            &format!("PATH={path}"),
            "VAULT_TOKEN=token",
            "LADE_VAULT_HTTP=1",
            "fish",
            "tests/test_vault.fish",
        ],
    );
}
