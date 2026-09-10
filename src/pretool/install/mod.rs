//! Optional installation of the `lade hook` interceptor into the agents that
//! support `preToolUse` shell hooks (Cursor, Claude Code, Codex, OpenCode).
//!
//! `lade setup` writes pre-exec (this shell) and, inside a git repo, pre-tool
//! hooks for this repo. No git: this shell only. `--cursor` and friends skip
//! the agent confirm.

mod agent;
mod inspect;
mod locate;
mod merge;
mod merge_json;
mod offer;
mod paths;
mod skill;
mod ui;
mod write;

#[cfg(test)]
mod tests;

pub use inspect::{HookLocation, PretoolAgentStatus, PretoolStatus, inspect};
pub(crate) use offer::setup;
pub(crate) use ui::print_setup;
pub(crate) use write::teardown;
pub use write::{Scope, install_scoped, refresh_installed, uninstall_scoped};

#[cfg(test)]
pub(crate) use write::refresh_at;
