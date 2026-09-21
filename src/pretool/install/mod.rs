//! Optional installation of the `lade hook` interceptor into harnesses that
//! support `preToolUse` shell hooks (Cursor, Claude Code, Codex, OpenCode).
//!
//! `lade setup` is this git repo. First-time shell wrap only. `--harness`
//! skips the harness confirm.

mod agent;
mod inspect;
mod locate;
mod merge;
mod merge_json;
mod offer;
mod paths;
mod ui;
mod write;

#[cfg(test)]
mod tests;

pub use inspect::{HookLocation, PretoolAgentStatus, PretoolStatus, inspect};
pub(crate) use offer::setup;
pub(crate) use ui::{print_setup, print_shell_hook, print_teardown};
pub(crate) use write::teardown;
pub use write::{Scope, install_scoped, refresh_installed, uninstall_scoped};

#[cfg(test)]
pub(crate) use write::refresh_at;
