use std::process::Command;

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
    require_cmds(&["bash", "zsh", "fish"]);

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

#[tokio::test]
async fn test_sh_provider_integration() {
    let mut providers = lade_sdk::Providers::new();
    providers
        .add("sh://echo integration-test".to_string())
        .unwrap();
    let (hydration, _) = providers
        .resolve(
            std::path::Path::new("."),
            &std::collections::HashMap::new(),
            &lade_sdk::Warnings::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        hydration.get("sh://echo integration-test").unwrap(),
        "integration-test"
    );
}
