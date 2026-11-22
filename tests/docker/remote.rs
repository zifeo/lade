use std::process::Command;
use tempfile::tempdir;

/// A real sshd in a container: `tests/remote_sshd.bash` links it to this
/// build and checks the remote resolves through it.
#[test]
fn a_remote_behind_a_real_sshd_resolves_through_the_link() {
    // A temp HOME, so ssh reads no key, agent or config of the developer's.
    let home = tempdir().expect("home");
    let docker_config = std::env::var("DOCKER_CONFIG")
        .unwrap_or_else(|_| format!("{}/.docker", std::env::var("HOME").expect("developer HOME")));
    let output = Command::new("env")
        .args([
            "-i".to_string(),
            format!("PATH={}", std::env::var("PATH").unwrap_or_default()),
            format!("HOME={}", home.path().display()),
            format!("DOCKER_CONFIG={docker_config}"),
            "bash".to_string(),
            "tests/remote_sshd.bash".to_string(),
            env!("CARGO_BIN_EXE_lade").to_string(),
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawn tests/remote_sshd.bash");
    assert!(
        output.status.success(),
        "tests/remote_sshd.bash failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
