use crate::preexec::Shell;
use crate::redact::Redactor;
use anyhow::Result;
use std::{collections::HashMap, path::Path, process::Stdio, sync::Arc};

pub fn run(
    shell: &Shell,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Arc<Redactor>,
    after_spawn: impl FnOnce(),
) -> Result<i32> {
    // Inherit stdin directly so the child reads the parent's fd. A `piped`
    // stdin forwarded by a helper thread risks SIGPIPE (SIG_DFL at startup
    // kills the process) when the child exits before consuming forwarded
    // bytes, which is observable on Linux CI.
    let watch = crate::child_signals::ChildWatch::new();
    let spawned = super::spawn_child(shell, command, env, cwd, |cmd| {
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(())
    });
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            after_spawn();
            return Err(error.into());
        }
    };
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
        // Unlocked: Lade's own log lines (the diary write) share stderr.
        redactor_stderr
            .stream(child_stderr, &mut std::io::stderr())
            .ok();
    });
    // After the readers start, so a slow diary never holds the output back.
    after_spawn();

    let status = child.wait()?;
    watch.clear_pid();
    stdout_thread.join().ok();
    stderr_thread.join().ok();

    Ok(status.code().unwrap_or(1))
}
