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

fn lade_bin() -> &'static str {
    env!("CARGO_BIN_EXE_lade")
}

fn run_cmd(cmd: &str, args: &[&str], home: &std::path::Path) {
    let output = Command::new(cmd)
        .args(args)
        .env("HOME", home)
        .env("LADE_CONFIG_PATH", home.join("lade-config.json"))
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

    let path = shell_first_path();
    let lade = lade_bin();
    // `env -i` drops HOME. Without a replacement, UserDirs falls back to
    // the real home and `lade set` refreshes hooks under this crate.
    let home = tempfile::tempdir().unwrap();
    let home_str = home.path().to_str().unwrap();
    let config = home.path().join("lade-config.json");
    let config_str = config.to_str().unwrap();
    run_cmd(
        "env",
        &[
            "-i",
            &format!("PATH={path}"),
            &format!("HOME={home_str}"),
            &format!("LADE_CONFIG_PATH={config_str}"),
            &format!("LADE_BIN={lade}"),
            "TEST=ok",
            "bash",
            "scripts/test.bash",
        ],
        home.path(),
    );
    run_cmd(
        "env",
        &[
            "-i",
            &format!("PATH={path}"),
            &format!("HOME={home_str}"),
            &format!("LADE_CONFIG_PATH={config_str}"),
            &format!("LADE_BIN={lade}"),
            "TEST=ok",
            "zsh",
            "scripts/test.zsh",
        ],
        home.path(),
    );
    run_cmd(
        "env",
        &[
            "-i",
            &format!("PATH={path}"),
            &format!("HOME={home_str}"),
            &format!("LADE_CONFIG_PATH={config_str}"),
            &format!("LADE_BIN={lade}"),
            "TEST=ok",
            "fish",
            "scripts/test.fish",
        ],
        home.path(),
    );
}
