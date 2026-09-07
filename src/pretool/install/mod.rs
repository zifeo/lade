//! Optional installation of the `lade hook` interceptor into the agents that
//! support `preToolUse` shell hooks (Cursor, Claude Code, Codex, OpenCode).
//!
//! `lade install` writes user-scope hooks and skills. No flags: offer
//! agents whose home dir exists, and ask harness, then hook, then skill.
//! `--cursor` and friends skip the harness ask only. Merge keeps unrelated
//! keys.

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
pub use offer::install;
pub use write::{Scope, install_scoped, refresh_installed, uninstall, uninstall_scoped};

#[cfg(test)]
pub(crate) use write::refresh_at;
