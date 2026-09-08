use crate::redact::Redactor;
use crate::shell::Shell;
use anyhow::Result;
use std::{collections::HashMap, path::Path, sync::Arc};

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

pub fn run(
    ctx: &crate::context::InvocationContext,
    shell: &Shell,
    command: &str,
    mut env: HashMap<String, String>,
    cwd: &Path,
    redactor: Option<Redactor>,
) -> Result<i32> {
    for key in crate::shell::CHILD_UNSET {
        env.remove(key);
    }
    let mode = select_mode(
        redactor.is_some(),
        ctx.stdin_is_terminal,
        ctx.stdout_is_terminal,
    );
    match mode {
        Mode::Plain => run_plain(shell, command, env, cwd),
        Mode::Pty => {
            let redactor = Arc::new(redactor.unwrap());
            #[cfg(unix)]
            {
                pty::run(shell, command, env, cwd, redactor)
            }
            #[cfg(not(unix))]
            {
                piped::run(shell, command, env, cwd, redactor)
            }
        }
        Mode::Piped => piped::run(shell, command, env, cwd, Arc::new(redactor.unwrap())),
    }
}

fn prepare_child(
    shell: &Shell,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
) -> std::process::Command {
    let mut cmd = shell.prepare_command(command);
    cmd.current_dir(cwd);
    cmd.envs(std::env::vars());
    crate::shell::strip_child_protocol(&mut cmd);
    cmd.env_remove("BASH_ENV");
    cmd.env_remove("MISE_ENV");
    cmd.env_remove("MISE_SETTINGS");
    cmd.envs(env);
    cmd
}

fn run_plain(
    shell: &Shell,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
) -> Result<i32> {
    let watch = crate::child_signals::ChildWatch::new();
    // Stay in lade's session. A new session here would let the child take
    // the inherited TTY and hang it up on exit.
    let mut child = prepare_child(shell, command, env, cwd).spawn()?;
    watch.set_pid(child.id(), false);
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
