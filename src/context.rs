use std::io::IsTerminal;

use crate::args::Command;

/// How much interactive UI an invocation may emit.
///
/// `Hook` — `lade set` / `unset` run inside shell preexec/postexec. The shell
/// owns the TTY; stdin echo and line editing are unreliable (see
/// <https://github.com/fish-shell/fish-shell/issues/8484>). stdout is the shell
/// protocol (`export` / `unset`); stderr stays quiet for nudges.
///
/// `Interactive` — `lade inject` with both stdin and stderr attached to a TTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Hook,
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvocationContext {
    pub mode: UiMode,
    pub stdin_is_terminal: bool,
    pub stdout_is_terminal: bool,
    pub stderr_is_terminal: bool,
}

impl InvocationContext {
    pub fn from_command(command: &Command) -> Self {
        Self::with_tty(
            command,
            std::io::stdin().is_terminal(),
            std::io::stdout().is_terminal(),
            std::io::stderr().is_terminal(),
        )
    }

    pub fn with_tty(
        command: &Command,
        stdin_is_terminal: bool,
        stdout_is_terminal: bool,
        stderr_is_terminal: bool,
    ) -> Self {
        let mode = match command {
            Command::Inject(_) | Command::Approve { .. }
                if stderr_is_terminal && stdin_is_terminal =>
            {
                UiMode::Interactive
            }
            _ => UiMode::Hook,
        };
        Self {
            mode,
            stdin_is_terminal,
            stdout_is_terminal,
            stderr_is_terminal,
        }
    }

    pub fn is_interactive(&self) -> bool {
        self.mode == UiMode::Interactive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{DEFAULT_MASK_FORMAT, EvalCommand, InjectCommand};

    #[test]
    fn set_is_always_hook() {
        let ctx = InvocationContext::with_tty(
            &Command::Set(EvalCommand {
                commands: vec!["x".into()],
            }),
            true,
            true,
            true,
        );
        assert_eq!(ctx.mode, UiMode::Hook);
        assert!(!ctx.is_interactive());
    }

    #[test]
    fn unset_is_always_hook() {
        let ctx = InvocationContext::with_tty(
            &Command::Unset(EvalCommand {
                commands: vec!["x".into()],
            }),
            true,
            true,
            true,
        );
        assert_eq!(ctx.mode, UiMode::Hook);
    }

    #[test]
    fn inject_without_tty_is_hook() {
        let ctx = InvocationContext::with_tty(
            &Command::Inject(InjectCommand {
                no_mask: false,
                mask_format: DEFAULT_MASK_FORMAT.into(),
                commands: vec!["x".into()],
            }),
            false,
            false,
            false,
        );
        assert_eq!(ctx.mode, UiMode::Hook);
    }

    #[test]
    fn status_is_hook() {
        let ctx = InvocationContext::with_tty(
            &Command::Status(crate::args::StatusCommand {
                all: false,
                json: false,
            }),
            true,
            true,
            true,
        );
        assert_eq!(ctx.mode, UiMode::Hook);
    }

    #[test]
    fn approve_is_hook_without_tty() {
        let ctx =
            InvocationContext::with_tty(&Command::Approve { code: None }, false, false, false);
        assert_eq!(ctx.mode, UiMode::Hook);
    }
}
