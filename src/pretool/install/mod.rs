//! Optional installation of the `lade hook` interceptor into the agents that
//! support `preToolUse` shell hooks (Cursor, Claude Code, Codex, OpenCode).
//!
//! `lade install` writes pre-exec (this shell) and pre-tool (hook and
//! skill together). A git cwd defaults to the repo. No git defaults to
//! this machine. `--cursor` and friends skip the agent confirm.

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

pub use inspect::{
    HookLocation, PretoolAgentStatus, PretoolStatus, SkillsStatus, inspect, inspect_skills,
};
pub(crate) use offer::install;
pub(crate) use ui::print_setup;
pub(crate) use write::uninstall;
pub use write::{Scope, install_scoped, refresh_installed, uninstall_scoped};

#[cfg(test)]
pub(crate) use write::refresh_at;
