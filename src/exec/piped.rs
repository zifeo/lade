use crate::redact::Redactor;
use anyhow::Result;
use std::{
    collections::HashMap,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
};

pub fn run(
    shell: &str,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Arc<Redactor>,
) -> Result<i32> {
    // Inherit stdin directly so the child reads the parent's fd. A `piped`
    // stdin forwarded by a helper thread risks SIGPIPE (SIG_DFL at startup
    // kills the process) when the child exits before consuming forwarded
    // bytes, which is observable on Linux CI.
    let mut child = Command::new(shell)
        .args(["-c", command])
        .current_dir(cwd)
        .envs(std::env::vars())
        .envs(env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let child_stdout = child.stdout.take().unwrap();
    let child_stderr = child.stderr.take().unwrap();

    let redactor_stderr = Arc::clone(&redactor);
    let stdout_thread = std::thread::spawn(move || {
        redactor
            .stream(child_stdout, &mut std::io::stdout().lock())
            .ok();
    });
    let stderr_thread = std::thread::spawn(move || {
        redactor_stderr
            .stream(child_stderr, &mut std::io::stderr().lock())
            .ok();
    });

    let status = child.wait()?;
    stdout_thread.join().ok();
    stderr_thread.join().ok();

    Ok(status.code().unwrap_or(1))
}
