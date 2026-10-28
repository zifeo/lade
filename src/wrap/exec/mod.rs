use crate::preexec::Shell;
use crate::redact::Redactor;
use anyhow::Result;
use std::{collections::HashMap, path::Path, sync::Arc};

mod path;
mod piped;
#[cfg(unix)]
mod pty;

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Plain,
    Pty,
    Piped,
}

fn select_mode(has_redactor: bool, stdin_tty: bool, stdout_tty: bool) -> Mode {
    if !has_redactor {
        return Mode::Plain;
    }
    if stdin_tty && stdout_tty {
        Mode::Pty
    } else {
        Mode::Piped
    }
}

/// `after_spawn` runs once the child is started, or failed to start, and
/// before Lade waits for it.
pub fn run(
    ctx: &crate::context::InvocationContext,
    shell: &Shell,
    command: &str,
    mut env: HashMap<String, String>,
    cwd: &Path,
    redactor: Option<Redactor>,
    after_spawn: impl FnOnce(),
) -> Result<i32> {
    for key in crate::preexec::CHILD_UNSET {
        env.remove(key);
    }
    let mode = select_mode(
        redactor.is_some(),
        ctx.stdin_is_terminal,
        ctx.stdout_is_terminal,
    );
    match mode {
        Mode::Plain => run_plain(shell, command, env, cwd, after_spawn),
        Mode::Pty => {
            let redactor = Arc::new(redactor.unwrap());
            #[cfg(unix)]
            {
                pty::run(shell, command, env, cwd, redactor, after_spawn)
            }
            #[cfg(not(unix))]
            {
                piped::run(shell, command, env, cwd, redactor, after_spawn)
            }
        }
        Mode::Piped => piped::run(
            shell,
            command,
            env,
            cwd,
            Arc::new(redactor.unwrap()),
            after_spawn,
        ),
    }
}

/// Starts `command` in `shell`, which reads the whole line. `stdio` wires
/// the child's streams.
fn spawn_child(
    shell: &Shell,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    stdio: impl Fn(&mut std::process::Command) -> std::io::Result<()>,
) -> std::io::Result<std::process::Child> {
    let path = env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok());
    // A mise shim standing in for the shell drops pinned dirs from PATH
    // before the script runs, so the script sets PATH again first.
    let script = match &path {
        Some(path) => format!(
            "{}\n{command}",
            shell.set(HashMap::from([("PATH".to_string(), path.clone())]))
        ),
        None => command.to_string(),
    };
    // A bare name with PATH in the child env makes std fork the whole
    // process and search PATH in the child: 20 to 30 ms under load. The
    // same lookup here lets it posix_spawn the path.
    let mut cmd = match path.and_then(|path| path::resolve(shell.bin(), &path)) {
        Some(program) => {
            use std::os::unix::process::CommandExt;
            let mut cmd = std::process::Command::new(program);
            cmd.arg0(shell.bin())
                .args(shell.noninteractive_args(&script));
            cmd
        }
        None => shell.prepare_command(&script),
    };
    child_env(&mut cmd, env, cwd);
    stdio(&mut cmd)?;
    let child = cmd.spawn();
    log_spawn(shell.bin());
    child
}

fn log_spawn(how: &str) {
    if let Some(started) = crate::STARTED.get() {
        crate::wrap::log_phase(&format!("main to spawn ({how})"), *started);
    }
}

fn child_env(cmd: &mut std::process::Command, env: HashMap<String, String>, cwd: &Path) {
    cmd.current_dir(cwd);
    cmd.envs(std::env::vars());
    crate::preexec::strip_child_protocol(cmd);
    cmd.env_remove("BASH_ENV");
    cmd.env_remove("MISE_ENV");
    cmd.env_remove("MISE_SETTINGS");
    cmd.envs(env);
}

fn run_plain(
    shell: &Shell,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    after_spawn: impl FnOnce(),
) -> Result<i32> {
    let watch = crate::child_signals::ChildWatch::new();
    // Stay in lade's session. A new session here would let the child take
    // the inherited TTY and hang it up on exit.
    let spawned = spawn_child(shell, command, env, cwd, |_| Ok(()));
    if let Ok(child) = &spawned {
        watch.set_pid(child.id(), false);
    }
    after_spawn();
    let mut child = spawned?;
    let status = child.wait()?;
    watch.clear_pid();
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_mode_plain_without_redactor() {
        assert_eq!(select_mode(false, true, true), Mode::Plain);
        assert_eq!(select_mode(false, false, false), Mode::Plain);
    }

    #[test]
    fn select_mode_pty_only_when_both_tty() {
        assert_eq!(select_mode(true, true, true), Mode::Pty);
    }

    #[test]
    fn select_mode_piped_when_stdin_not_tty() {
        assert_eq!(select_mode(true, false, true), Mode::Piped);
    }

    #[test]
    fn select_mode_piped_when_stdout_not_tty() {
        assert_eq!(select_mode(true, true, false), Mode::Piped);
        assert_eq!(select_mode(true, false, false), Mode::Piped);
    }
}
