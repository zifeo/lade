use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::agent::Agent;

pub(super) fn leftover_json_hook(agent: Agent, home: &Path) -> Result<Option<(PathBuf, String)>> {
    let Some(path) = agent.legacy_json_path(home) else {
        return Ok(None);
    };
    leftover_hook_file(Agent::Claude, &path)
}

/// The shared file, which Claude reads only in the directory it starts in:
/// a Lade hook there misses a session started in a subdirectory. Enable
/// moves it to `settings.local.json`; disable strips both.
pub(super) fn claude_shared_hook(dest: &Path) -> Result<Option<(PathBuf, String)>> {
    leftover_hook_file(Agent::Claude, &dest.join(".claude").join("settings.json"))
}

fn leftover_hook_file(agent: Agent, path: &Path) -> Result<Option<(PathBuf, String)>> {
    match fs::read_to_string(path) {
        Ok(content) if agent.has_hook(&content)? => Ok(Some((path.to_path_buf(), content))),
        _ => Ok(None),
    }
}

pub(super) fn hook_state(path: &Path, agent: Agent, expected: Option<&str>) -> (bool, bool) {
    let Ok(content) = fs::read_to_string(path) else {
        return (false, false);
    };
    let installed = agent.has_hook(&content).unwrap_or(false);
    let current = installed
        && expected
            .is_some_and(|command| agent.hook_uses_command(&content, command).unwrap_or(false));
    (installed, current)
}

/// One project dest: the git root. Home is never a dest.
pub(super) fn project_dest(home: &Path, cwd: &Path) -> Option<PathBuf> {
    let root = crate::catalog::git_root(cwd)?;
    if same_path(&root, home) {
        None
    } else {
        Some(root)
    }
}

/// Project hook file at `project_dest`. Home configs stay global.
pub(super) fn find_project(agent: Agent, home: &Path, cwd: &Path) -> Result<(PathBuf, bool)> {
    let Some(dest) = project_dest(home, cwd) else {
        return Ok((canonical_project_path(agent, cwd), false));
    };
    let path = canonical_project_path(agent, &dest);
    let installed = path.is_file() && hook_state(&path, agent, None).0;
    Ok((path, installed))
}

pub(super) fn canonical_project_path(agent: Agent, dest: &Path) -> PathBuf {
    match agent {
        Agent::Cursor => dest.join(".cursor").join("hooks.json"),
        // Read from the git root even when Claude starts in a subdirectory.
        Agent::Claude => dest.join(".claude").join("settings.local.json"),
        Agent::Codex => dest.join(".codex").join("hooks.json"),
        Agent::OpenCode => dest
            .join(".opencode")
            .join("plugins")
            .join("lade-pretool.js"),
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
