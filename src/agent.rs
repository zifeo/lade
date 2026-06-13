//! Best-effort detection of whether an AI coding agent (rather than a human) is
//! driving the CLI on the DIRECT `lade inject` / `lade <cmd>` path.
//!
//! The hook path (`lade hook`) deliberately does NOT use this: an agent
//! literally invokes the hook, so that context is agent-by-construction (see
//! the module doc-comment in `src/hook/mod.rs`). Detection is only needed where TTY
//! heuristics alone cannot tell an agent apart from CI or an interactive human.
//!
//! Detection precedence (first match wins):
//! 1. `AI_AGENT` — Vercel `@vercel/detect-agent` convention; the value is the
//!    tool name, e.g. `claude-code`, `cursor-cli`.
//! 2. `AGENT` — community convention modeled on `CI=true`; proposed in
//!    <https://github.com/agentsmd/agents.md/issues/136>; adopted by Goose,
//!    Amp, and Bun.
//! 3. tool-specific fallbacks:
//!    - `CLAUDECODE=1` — Claude Code (<https://code.claude.com/docs/en/env-vars>).
//!    - `CURSOR_AGENT` — Cursor's agent CLI.
//!    - `COPILOT_MODEL` — GitHub Copilot.
//!    - `CURSOR_VERSION` — Cursor; NOTE this is ALSO set in a human's Cursor
//!      terminal, so it is ambiguous and only used as a last resort.
//!    - Gemini CLI has NO dedicated detection variable, so it cannot be detected.

use std::env;

fn nonempty(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.is_empty())
}

/// Returns the detected agent name, or `None` when no agent signal is present.
pub fn detect_agent() -> Option<String> {
    if let Some(name) = nonempty("AI_AGENT") {
        return Some(name);
    }
    if let Some(name) = nonempty("AGENT") {
        return Some(name);
    }
    if env::var("CLAUDECODE").ok().as_deref() == Some("1") {
        return Some("claude-code".to_string());
    }
    if nonempty("CURSOR_AGENT").is_some() {
        return Some("cursor".to_string());
    }
    if nonempty("COPILOT_MODEL").is_some() {
        return Some("copilot".to_string());
    }
    // Ambiguous: CURSOR_VERSION is also present in a human's Cursor terminal.
    if nonempty("CURSOR_VERSION").is_some() {
        return Some("cursor".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [&str; 6] = [
        "AI_AGENT",
        "AGENT",
        "CLAUDECODE",
        "CURSOR_AGENT",
        "COPILOT_MODEL",
        "CURSOR_VERSION",
    ];

    fn cleared() -> Vec<(&'static str, Option<&'static str>)> {
        ALL.iter().map(|k| (*k, None)).collect()
    }

    fn with(key: &'static str, value: &'static str) -> Vec<(&'static str, Option<&'static str>)> {
        let mut vars = cleared();
        for entry in &mut vars {
            if entry.0 == key {
                entry.1 = Some(value);
            }
        }
        vars
    }

    #[test]
    fn none_when_no_signal() {
        temp_env::with_vars(cleared(), || assert_eq!(detect_agent(), None));
    }

    #[test]
    fn ai_agent_takes_precedence() {
        temp_env::with_vars(
            [
                ("AI_AGENT", Some("claude-code")),
                ("AGENT", Some("goose")),
                ("CLAUDECODE", Some("1")),
                ("CURSOR_AGENT", None),
                ("COPILOT_MODEL", None),
                ("CURSOR_VERSION", None),
            ],
            || assert_eq!(detect_agent().as_deref(), Some("claude-code")),
        );
    }

    #[test]
    fn agent_beats_tool_specific() {
        temp_env::with_vars(
            [
                ("AI_AGENT", None),
                ("AGENT", Some("amp")),
                ("CLAUDECODE", Some("1")),
                ("CURSOR_AGENT", None),
                ("COPILOT_MODEL", None),
                ("CURSOR_VERSION", None),
            ],
            || assert_eq!(detect_agent().as_deref(), Some("amp")),
        );
    }

    #[test]
    fn claudecode_flag() {
        temp_env::with_vars(with("CLAUDECODE", "1"), || {
            assert_eq!(detect_agent().as_deref(), Some("claude-code"));
        });
    }

    #[test]
    fn claudecode_non_one_is_ignored() {
        temp_env::with_vars(with("CLAUDECODE", "0"), || {
            assert_eq!(detect_agent(), None);
        });
    }

    #[test]
    fn cursor_agent_detected() {
        temp_env::with_vars(with("CURSOR_AGENT", "1"), || {
            assert_eq!(detect_agent().as_deref(), Some("cursor"));
        });
    }

    #[test]
    fn copilot_detected() {
        temp_env::with_vars(with("COPILOT_MODEL", "gpt-x"), || {
            assert_eq!(detect_agent().as_deref(), Some("copilot"));
        });
    }

    #[test]
    fn cursor_version_is_last_resort() {
        temp_env::with_vars(with("CURSOR_VERSION", "1.0"), || {
            assert_eq!(detect_agent().as_deref(), Some("cursor"));
        });
    }
}
