use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::agent::Agent;
use super::skill::{is_lade_skill, skill_is_current};

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

pub(super) fn skill_state(path: &Path) -> (bool, bool) {
    let Ok(content) = fs::read_to_string(path) else {
        return (false, false);
    };
    let installed = is_lade_skill(&content);
    (installed, installed && skill_is_current(&content))
}

/// Walk from `cwd` toward `$HOME` looking for a project-local hook file.
/// Home-level agent configs stay in `global`.
pub(super) fn find_project(agent: Agent, home: &Path, cwd: &Path) -> Result<(PathBuf, bool)> {
    let under_home = cwd.starts_with(home);
    let mut dir = cwd.to_path_buf();
    loop {
        if dir == home {
            break;
        }
        let files = project_files(agent, &dir);
        for path in &files {
            if path.is_file() && hook_state(path, agent, None).0 {
                return Ok((path.clone(), true));
            }
        }
        if let Some(found) = files.into_iter().find(|path| path.is_file()) {
            return Ok((found, false));
        }
        match dir.parent() {
            Some(parent) if under_home => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    Ok((canonical_project_path(agent, cwd), false))
}

pub(super) fn find_project_skill(agent: Agent, home: &Path, cwd: &Path) -> Result<(PathBuf, bool)> {
    let under_home = cwd.starts_with(home);
    let mut dir = cwd.to_path_buf();
    loop {
        if dir == home {
            break;
        }
        let path = project_skill_file(agent, &dir);
        if path.is_file() && skill_state(&path).0 {
            return Ok((path, true));
        }
        if path.is_file() {
            return Ok((path, false));
        }
        match dir.parent() {
            Some(parent) if under_home => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    Ok((project_skill_file(agent, cwd), false))
}

pub(super) fn find_agents_skill(home: &Path, cwd: &Path) -> Option<PathBuf> {
    let under_home = cwd.starts_with(home);
    let mut dir = cwd.to_path_buf();
    loop {
        if dir == home {
            break;
        }
        let path = dir
            .join(".agents")
            .join("skills")
            .join("lade")
            .join("SKILL.md");
        if path.is_file() {
            return Some(path);
        }
        match dir.parent() {
            Some(parent) if under_home => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    None
}

pub(super) fn project_skill_file(agent: Agent, dir: &Path) -> PathBuf {
    match agent {
        Agent::Cursor => dir
            .join(".cursor")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
        Agent::Claude => dir
            .join(".claude")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
        Agent::Codex => dir
            .join(".codex")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
        Agent::OpenCode => dir
            .join(".opencode")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
    }
}

pub(super) fn project_files(agent: Agent, dir: &Path) -> Vec<PathBuf> {
    match agent {
        Agent::Cursor => vec![dir.join(".cursor").join("hooks.json")],
        Agent::Claude => vec![
            dir.join(".claude").join("settings.local.json"),
            dir.join(".claude").join("settings.json"),
        ],
        Agent::Codex => vec![dir.join(".codex").join("hooks.json")],
        Agent::OpenCode => vec![
            dir.join(".opencode")
                .join("plugins")
                .join("lade-pretool.js"),
        ],
    }
}

pub(super) fn canonical_project_path(agent: Agent, cwd: &Path) -> PathBuf {
    match agent {
        Agent::Cursor => cwd.join(".cursor").join("hooks.json"),
        Agent::Claude => cwd.join(".claude").join("settings.json"),
        Agent::Codex => cwd.join(".codex").join("hooks.json"),
        Agent::OpenCode => cwd
            .join(".opencode")
            .join("plugins")
            .join("lade-pretool.js"),
    }
}
