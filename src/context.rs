use std::io::IsTerminal;

use anyhow::Result;

use crate::args::Command;
use crate::audience::{self, Detection, UiMode, Via};
use crate::config::Audience;

/// TTY flags plus the Via / Audience / UI decision from [`audience::detect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvocationContext {
    pub via: Via,
    pub audience: Audience,
    pub mode: UiMode,
    pub stdin_is_terminal: bool,
    pub stdout_is_terminal: bool,
    pub stderr_is_terminal: bool,
}

impl InvocationContext {
    pub fn from_command(command: &Command) -> Result<Self> {
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
    ) -> Result<Self> {
        let Detection { via, audience, ui } =
            audience::detect(command, stdin_is_terminal, stderr_is_terminal)?;
        Ok(Self {
            via,
            audience,
            mode: ui,
            stdin_is_terminal,
            stdout_is_terminal,
            stderr_is_terminal,
        })
    }

    pub fn is_interactive(&self) -> bool {
        self.mode == UiMode::Interactive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{DEFAULT_MASK_FORMAT, EvalCommand, InjectCommand};

    fn inject() -> Command {
        Command::Inject(InjectCommand {
            no_mask: false,
            mask_format: DEFAULT_MASK_FORMAT.into(),
            commands: vec!["x".into()],
        })
    }

    fn cleared() -> Vec<(&'static str, Option<&'static str>)> {
        vec![
            (crate::shell::LADE_VIA, None),
            ("AI_AGENT", None),
            ("AGENT", None),
            ("CLAUDECODE", None),
            ("CURSOR_AGENT", None),
            ("COPILOT_MODEL", None),
            ("CURSOR_VERSION", None),
        ]
    }

    #[test]
    fn set_is_always_quiet() {
        temp_env::with_vars(cleared(), || {
            let ctx = InvocationContext::with_tty(
                &Command::Set(EvalCommand {
                    commands: vec!["x".into()],
                }),
                true,
                true,
                true,
            )
            .unwrap();
            assert_eq!(ctx.mode, UiMode::Quiet);
            assert_eq!(ctx.via, Via::Preexec);
            assert!(!ctx.is_interactive());
        });
    }

    #[test]
    fn unset_is_always_quiet() {
        temp_env::with_vars(cleared(), || {
            let ctx = InvocationContext::with_tty(
                &Command::Unset(EvalCommand {
                    commands: vec!["x".into()],
                }),
                true,
                true,
                true,
            )
            .unwrap();
            assert_eq!(ctx.mode, UiMode::Quiet);
        });
    }

    #[test]
    fn inject_without_tty_is_quiet() {
        temp_env::with_vars(cleared(), || {
            let ctx = InvocationContext::with_tty(&inject(), false, false, false).unwrap();
            assert_eq!(ctx.mode, UiMode::Quiet);
        });
    }

    #[test]
    fn inject_with_tty_is_interactive() {
        temp_env::with_vars(cleared(), || {
            let ctx = InvocationContext::with_tty(&inject(), true, true, true).unwrap();
            assert_eq!(ctx.mode, UiMode::Interactive);
            assert_eq!(ctx.audience, Audience::Human);
        });
    }

    #[test]
    fn status_is_quiet() {
        temp_env::with_vars(cleared(), || {
            let ctx = InvocationContext::with_tty(
                &Command::Status(crate::args::StatusCommand {
                    all: false,
                    json: false,
                }),
                true,
                true,
                true,
            )
            .unwrap();
            assert_eq!(ctx.mode, UiMode::Quiet);
        });
    }

    #[test]
    fn approve_is_quiet_without_tty() {
        temp_env::with_vars(cleared(), || {
            let ctx =
                InvocationContext::with_tty(&Command::Approve { code: None }, false, false, false)
                    .unwrap();
            assert_eq!(ctx.mode, UiMode::Quiet);
        });
    }
}
