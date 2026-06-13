use crate::redact::Redactor;
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
    shell: &str,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Option<Redactor>,
) -> Result<i32> {
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

fn run_plain(shell: &str, command: &str, env: HashMap<String, String>, cwd: &Path) -> Result<i32> {
    let status = std::process::Command::new(shell)
        .args(["-c", command])
        .current_dir(cwd)
        .envs(std::env::vars())
        .envs(env)
        .status()?;
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
