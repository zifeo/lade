use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::agent::Agent;

pub(super) fn leftover_json_hook(agent: Agent, home: &Path) -> Result<Option<(PathBuf, String)>> {
    let Some(path) = agent.legacy_json_path(home) else {
        return Ok(None);
    };
    match fs::read_to_string(&path) {
        Ok(content) if Agent::Claude.has_hook(&content)? => Ok(Some((path, content))),
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
        // Claude reads it at the git root even from a subdirectory, and at
        // the main checkout's root from a linked worktree.
        Agent::Claude => main_checkout(dest)
            .join(".claude")
            .join("settings.local.json"),
        Agent::Codex => dest.join(".codex").join("hooks.json"),
        Agent::OpenCode => dest
            .join(".opencode")
            .join("plugins")
            .join("lade-pretool.js"),
    }
}

/// The main checkout of the linked worktree at `dest`, else `dest`. Only a
/// linked worktree's gitdir has a `commondir`: a submodule's does not.
pub(super) fn main_checkout(dest: &Path) -> PathBuf {
    linked_main_checkout(dest).unwrap_or_else(|| dest.to_path_buf())
}

fn linked_main_checkout(dest: &Path) -> Option<PathBuf> {
    let gitfile = dest.join(".git");
    if !gitfile.is_file() {
        return None;
    }
    let text = fs::read_to_string(&gitfile).ok()?;
    let gitdir = dest.join(
        text.lines()
            .find_map(|line| line.strip_prefix("gitdir:"))?
            .trim(),
    );
    let common = fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = fs::canonicalize(gitdir.join(common.trim())).ok()?;
    // A bare repo's worktrees have no main checkout.
    if common.file_name()? != ".git" {
        return None;
    }
    common.parent().map(Path::to_path_buf)
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
