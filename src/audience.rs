//! Single decision for Via, Audience, and UI on every Lade invocation.
//!
//! Callers pass `ctx.audience` into `collect_for`. They do not pick Human/Agent
//! themselves.

use anyhow::{Result, bail};

use crate::args::Command;
use crate::config::Audience;
use crate::shell::{LADE_VIA, LADE_VIA_PREEXEC, LADE_VIA_PRETOOL};

/// How this process was reached. Empty means neither interceptor classified it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Preexec,
    Pretool,
    Unset,
}

impl Via {
    /// Value written to `LADE_VIA` on the child command. Preexec already
    /// exports `preexec` into the shell; `--pretool` does the same for inject.
    /// Unset means strip so a leftover parent value does not leak.
    pub fn child_stamp(self) -> Option<&'static str> {
        match self {
            Via::Pretool => Some(LADE_VIA_PRETOOL),
            Via::Preexec => Some(LADE_VIA_PREEXEC),
            Via::Unset => None,
        }
    }
}

/// Whether this invocation may prompt on stdin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Quiet,
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detection {
    pub via: Via,
    pub audience: Audience,
    pub ui: UiMode,
}

/// Classify this invocation. `--pretool` wins, then `LADE_VIA`, then the
/// subcommand, then agent env signals. UI is an output: Interactive only for
/// human inject/approve with both stdin and stderr attached to a TTY.
pub fn detect(
    command: &Command,
    pretool: bool,
    stdin_is_terminal: bool,
    stderr_is_terminal: bool,
) -> Result<Detection> {
    let via = via(command, pretool)?;
    let audience = match via {
        Via::Pretool => Audience::Agent,
        Via::Preexec => Audience::Human,
        Via::Unset => {
            if agent_signal().is_some() {
                Audience::Agent
            } else {
                Audience::Human
            }
        }
    };
    let can_prompt = matches!(command, Command::Inject(_) | Command::Approve { .. })
        && stdin_is_terminal
        && stderr_is_terminal
        && audience == Audience::Human;
    let ui = if can_prompt {
        UiMode::Interactive
    } else {
        UiMode::Quiet
    };
    Ok(Detection { via, audience, ui })
}

fn via(command: &Command, pretool: bool) -> Result<Via> {
    if pretool {
        return Ok(Via::Pretool);
    }
    match std::env::var(LADE_VIA) {
        Ok(value) if value == LADE_VIA_PRETOOL => return Ok(Via::Pretool),
        Ok(value) if value == LADE_VIA_PREEXEC => return Ok(Via::Preexec),
        Ok(value) if !value.is_empty() => {
            bail!("LADE_VIA must be '{LADE_VIA_PREEXEC}' or '{LADE_VIA_PRETOOL}', got '{value}'")
        }
        Ok(_) | Err(_) => {}
    }
    Ok(match command {
        Command::Set(_) | Command::Unset(_) => Via::Preexec,
        Command::Hook => Via::Pretool,
        _ => Via::Unset,
    })
}

/// Best-effort agent name from env. Used only when Via is unset.
///
/// Precedence (first match wins):
/// 1. `AI_AGENT` — Vercel `@vercel/detect-agent`; value is the tool name.
/// 2. `AGENT` — community convention ([agents.md#136](https://github.com/agentsmd/agents.md/issues/136)).
/// 3. `CLAUDECODE=1`, `CURSOR_AGENT`, `COPILOT_MODEL`.
///
/// `CURSOR_VERSION` is not used: Cursor sets it in human terminals too.
fn agent_signal() -> Option<String> {
    if let Some(name) = nonempty("AI_AGENT") {
        return Some(name);
    }
    if let Some(name) = nonempty("AGENT") {
        return Some(name);
    }
    if std::env::var("CLAUDECODE").ok().as_deref() == Some("1") {
        return Some("claude-code".to_string());
    }
    if nonempty("CURSOR_AGENT").is_some() {
        return Some("cursor".to_string());
    }
    if nonempty("COPILOT_MODEL").is_some() {
        return Some("copilot".to_string());
    }
    None
}

fn nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{DEFAULT_MASK_FORMAT, EvalCommand, InjectCommand};

    const SIGNALS: [&str; 6] = [
        "AI_AGENT",
        "AGENT",
        "CLAUDECODE",
        "CURSOR_AGENT",
        "COPILOT_MODEL",
        "CURSOR_VERSION",
    ];

    fn cleared_signals() -> Vec<(&'static str, Option<&'static str>)> {
        let mut vars: Vec<_> = SIGNALS.iter().map(|k| (*k, None)).collect();
        vars.push((LADE_VIA, None));
        vars
    }

    fn inject() -> Command {
        Command::Inject(InjectCommand {
            no_mask: false,
            mask_format: DEFAULT_MASK_FORMAT.into(),
            commands: vec!["x".into()],
        })
    }

    fn set_cmd() -> Command {
        Command::Set(EvalCommand {
            commands: vec!["x".into()],
        })
    }

    #[test]
    fn via_pretool_is_agent_quiet() {
        temp_env::with_vars(
            cleared_signals()
                .into_iter()
                .map(|(k, v)| {
                    if k == LADE_VIA {
                        (k, Some(LADE_VIA_PRETOOL))
                    } else {
                        (k, v)
                    }
                })
                .collect::<Vec<_>>(),
            || {
                let d = detect(&inject(), false, true, true).unwrap();
                assert_eq!(d.via, Via::Pretool);
                assert_eq!(d.audience, Audience::Agent);
                assert_eq!(d.ui, UiMode::Quiet);
            },
        );
    }

    #[test]
    fn via_preexec_overrides_cursor_version() {
        temp_env::with_vars(
            [
                (LADE_VIA, Some(LADE_VIA_PREEXEC)),
                ("CURSOR_VERSION", Some("1.0")),
                ("AI_AGENT", None),
                ("AGENT", None),
                ("CLAUDECODE", None),
                ("CURSOR_AGENT", None),
                ("COPILOT_MODEL", None),
            ],
            || {
                let d = detect(&inject(), false, true, true).unwrap();
                assert_eq!(d.via, Via::Preexec);
                assert_eq!(d.audience, Audience::Human);
                assert_eq!(d.ui, UiMode::Interactive);
            },
        );
    }

    #[test]
    fn inject_via_flag_is_pretool_agent_quiet() {
        temp_env::with_vars(cleared_signals(), || {
            let d = detect(&inject(), true, true, true).unwrap();
            assert_eq!(d.via, Via::Pretool);
            assert_eq!(d.audience, Audience::Agent);
            assert_eq!(d.ui, UiMode::Quiet);
        });
    }

    #[test]
    fn via_flag_on_status_is_pretool_agent() {
        temp_env::with_vars(cleared_signals(), || {
            let d = detect(
                &Command::Status(crate::args::StatusCommand {
                    all: false,
                    json: false,
                }),
                true,
                true,
                true,
            )
            .unwrap();
            assert_eq!(d.via, Via::Pretool);
            assert_eq!(d.audience, Audience::Agent);
            assert_eq!(d.ui, UiMode::Quiet);
        });
    }

    #[test]
    fn via_flag_wins_over_env() {
        temp_env::with_vars(
            cleared_signals()
                .into_iter()
                .map(|(k, v)| {
                    if k == LADE_VIA {
                        (k, Some(LADE_VIA_PREEXEC))
                    } else {
                        (k, v)
                    }
                })
                .collect::<Vec<_>>(),
            || {
                let d = detect(&inject(), true, true, true).unwrap();
                assert_eq!(d.via, Via::Pretool);
                assert_eq!(d.audience, Audience::Agent);
            },
        );
    }

    #[test]
    fn set_is_preexec_human_quiet() {
        temp_env::with_vars(cleared_signals(), || {
            let d = detect(&set_cmd(), false, true, true).unwrap();
            assert_eq!(d.via, Via::Preexec);
            assert_eq!(d.audience, Audience::Human);
            assert_eq!(d.ui, UiMode::Quiet);
        });
    }

    #[test]
    fn hook_is_pretool_agent_quiet() {
        temp_env::with_vars(cleared_signals(), || {
            let d = detect(&Command::Hook, false, false, false).unwrap();
            assert_eq!(d.via, Via::Pretool);
            assert_eq!(d.audience, Audience::Agent);
            assert_eq!(d.ui, UiMode::Quiet);
        });
    }

    #[test]
    fn empty_via_with_signal_is_agent_quiet() {
        temp_env::with_vars(
            [
                (LADE_VIA, None),
                ("AI_AGENT", None),
                ("AGENT", None),
                ("CLAUDECODE", None),
                ("CURSOR_AGENT", Some("1")),
                ("COPILOT_MODEL", None),
                ("CURSOR_VERSION", None),
            ],
            || {
                let d = detect(&inject(), false, true, true).unwrap();
                assert_eq!(d.via, Via::Unset);
                assert_eq!(d.audience, Audience::Agent);
                assert_eq!(d.ui, UiMode::Quiet);
            },
        );
    }

    #[test]
    fn empty_via_without_signal_inject_tty_is_human_interactive() {
        temp_env::with_vars(cleared_signals(), || {
            let d = detect(&inject(), false, true, true).unwrap();
            assert_eq!(d.via, Via::Unset);
            assert_eq!(d.audience, Audience::Human);
            assert_eq!(d.ui, UiMode::Interactive);
        });
    }

    #[test]
    fn cursor_version_alone_is_human() {
        temp_env::with_vars(
            [
                (LADE_VIA, None),
                ("AI_AGENT", None),
                ("AGENT", None),
                ("CLAUDECODE", None),
                ("CURSOR_AGENT", None),
                ("COPILOT_MODEL", None),
                ("CURSOR_VERSION", Some("1.0")),
            ],
            || {
                let d = detect(&inject(), false, true, true).unwrap();
                assert_eq!(d.via, Via::Unset);
                assert_eq!(d.audience, Audience::Human);
                assert_eq!(d.ui, UiMode::Interactive);
            },
        );
    }

    #[test]
    fn invalid_via_fails() {
        temp_env::with_var(LADE_VIA, Some("nope"), || {
            assert!(detect(&inject(), false, false, false).is_err());
        });
    }

    #[test]
    fn ai_agent_takes_precedence() {
        temp_env::with_vars(
            [
                (LADE_VIA, None),
                ("AI_AGENT", Some("claude-code")),
                ("AGENT", Some("goose")),
                ("CLAUDECODE", Some("1")),
                ("CURSOR_AGENT", None),
                ("COPILOT_MODEL", None),
                ("CURSOR_VERSION", None),
            ],
            || assert_eq!(agent_signal().as_deref(), Some("claude-code")),
        );
    }

    #[test]
    fn claudecode_non_one_is_ignored() {
        temp_env::with_vars(
            [
                (LADE_VIA, None),
                ("AI_AGENT", None),
                ("AGENT", None),
                ("CLAUDECODE", Some("0")),
                ("CURSOR_AGENT", None),
                ("COPILOT_MODEL", None),
                ("CURSOR_VERSION", None),
            ],
            || assert_eq!(agent_signal(), None),
        );
    }

    #[test]
    fn child_stamp_matches_via() {
        assert_eq!(Via::Pretool.child_stamp(), Some(LADE_VIA_PRETOOL));
        assert_eq!(Via::Preexec.child_stamp(), Some(LADE_VIA_PREEXEC));
        assert_eq!(Via::Unset.child_stamp(), None);
    }
}
