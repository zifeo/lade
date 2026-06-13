//! Stable, documented process exit codes for lade.
//!
//! These codes are the canonical reference for agents and CI driving lade; they
//! are kept stable across minor versions so callers can branch on them.
//!
//! Convention follows InfoQ "Patterns for AI Agent Driven CLIs"
//! (<https://www.infoq.com/articles/ai-agent-cli/>):
//! `0` success, `1-2` correctable user/config errors, `3-125` app-specific,
//! `128+n` terminated by signal `n`.
//!
//! Child passthrough: when lade wraps a command (`lade inject`), the child's
//! own exit code is propagated unchanged. Any non-zero code lade does not
//! itself produce therefore originates from the wrapped command.

/// Generic, correctable failure: config parse error, secret loader error, or an
/// `anyhow` error bubbling out of `main`.
pub const FAILURE: i32 = 1;

/// A disclaimer-protected command was invoked without approval, so secrets were
/// withheld (fail-closed). To proceed, re-run with the per-command code shown in
/// the message (`LADE_APPROVE=<code>`) or run `lade approve`. App-specific
/// (`3-125` range).
pub const DISCLAIMER_WITHHELD: i32 = 3;

/// Interrupted by the user (Ctrl-C / SIGINT). Mirrors the shell convention of
/// `128 + SIGINT(2)`.
pub const INTERRUPTED: i32 = 130;
