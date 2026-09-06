//! Single decision for Via, Audience, and UI on every Lade invocation.
//!
//! Callers pass `ctx.audience` into `collect_for`. They do not pick Human/Agent
//! themselves.

use anyhow::Result;

use crate::args::Command;
use crate::config::Audience;

/// How this process was reached. That is not the `lade unset` command.
///
/// Organic is a best-effort guess: both stdin and stderr are TTYs and no
/// agent env signal fired. An undetected agent on a TTY still looks organic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Preexec,
    Pretool,
    Mcp,
    Organic,
    Unknown,
}

impl Via {
    pub const PREEXEC: &'static str = "preexec";
    pub const PRETOOL: &'static str = "pretool";
    pub const MCP: &'static str = "mcp";

    /// Value stored on the ticket and in diary rows. Not written to child env.
    pub fn child_stamp(self) -> Option<&'static str> {
        match self {
            Via::Pretool => Some(Self::PRETOOL),
            Via::Preexec => Some(Self::PREEXEC),
            Via::Mcp | Via::Organic | Via::Unknown => None,
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

/// Classify this invocation. `--pretool` wins, then the subcommand, then
/// agent env signals. UI is an output: Interactive only for human
/// inject/approve with both stdin and stderr attached to a TTY.
pub fn detect(
    command: &Command,
    pretool: bool,
    stdin_is_terminal: bool,
    stderr_is_terminal: bool,
) -> Result<Detection> {
    let via = via(command, pretool);
    let via = match via {
        Via::Unknown if stdin_is_terminal && stderr_is_terminal && agent_signal().is_none() => {
            Via::Organic
        }
        other => other,
    };
    let audience = match via {
        Via::Pretool | Via::Mcp => Audience::Agent,
        Via::Preexec | Via::Organic => Audience::Human,
        Via::Unknown => {
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

fn via(command: &Command, pretool: bool) -> Via {
    if pretool {
        return Via::Pretool;
    }
    match command {
        Command::Set(_) | Command::Unset(_) => Via::Preexec,
        Command::Hook { .. } => Via::Pretool,
        Command::Mcp(_) => Via::Mcp,
        _ => Via::Unknown,
    }
}

/// Best-effort harness name from env. Used when Via is unknown and as
/// a fallback for diary `agent` metadata. Home-dir vars are ignored.
///
/// Precedence (first match wins): `AI_AGENT`, `AGENT`, then Claude /
/// Cursor / Codex / Pi / OpenCode / Copilot signals.
/// `CURSOR_VERSION` is ignored: Cursor sets it in human terminals too.
pub(crate) fn agent_signal() -> Option<String> {
    if let Some(name) = nonempty("AI_AGENT") {
        return Some(name);
    }
    if let Some(name) = nonempty("AGENT") {
        return Some(name);
    }
    if std::env::var("CLAUDECODE").ok().as_deref() == Some("1") || nonempty("CLAUDE_CODE").is_some()
    {
        return Some("claude".to_string());
    }
    if nonempty("CURSOR_AGENT").is_some()
        || std::env::var("CURSOR_EXTENSION_HOST_ROLE").ok().as_deref() == Some("agent-exec")
        || nonempty("CURSOR_SANDBOX").is_some()
    {
        return Some("cursor".to_string());
    }
    if nonempty("CODEX_THREAD_ID").is_some()
        || nonempty("CODEX_SANDBOX").is_some()
        || nonempty("CODEX_CI").is_some()
    {
        return Some("codex".to_string());
    }
    if nonempty("PI_MODEL").is_some() || nonempty("PI_SESSION_ID").is_some() {
        return Some("pi".to_string());
    }
    if nonempty("OPENCODE").is_some() || nonempty("OPENCODE_PID").is_some() {
        return Some("opencode".to_string());
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
mod tests;
