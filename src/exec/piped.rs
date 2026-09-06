use crate::redact::Redactor;
use crate::shell::Shell;
use anyhow::Result;
use std::{collections::HashMap, path::Path, process::Stdio, sync::Arc};

pub fn run(
    shell: &Shell,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Arc<Redactor>,
) -> Result<i32> {
    // Inherit stdin directly so the child reads the parent's fd. A `piped`
    // stdin forwarded by a helper thread risks SIGPIPE (SIG_DFL at startup
    // kills the process) when the child exits before consuming forwarded
    // bytes, which is observable on Linux CI.
    let watch = crate::child_signals::ChildWatch::new();
    let mut child = shell
        .prepare_command(command)
        .current_dir(cwd)
        .envs(std::env::vars())
        .env_remove(crate::shell::LADE_VIA)
        .env_remove("BASH_ENV")
        .envs(env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    watch.set_pid(child.id(), false);

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
    watch.clear_pid();
    stdout_thread.join().ok();
    stderr_thread.join().ok();

    Ok(status.code().unwrap_or(1))
}
