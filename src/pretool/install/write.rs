use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::agent::{AGENTS, Agent};
use super::locate::{
    canonical_project_path, find_agents_skill, find_project, find_project_skill, leftover_json_hook,
};
use super::paths::{home_dir, hook_command, tilde};
use super::skill::{SKILL_MD, is_lade_skill, skill_is_current};
use super::ui::report;

/// User home config versus the files in the current directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    User,
    Project,
}

/// Write one harness hook at `scope`. Creates parent dirs. Does not prompt.
pub fn install_scoped(scope: Scope, harness: &str) -> Result<()> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let line = write_scoped(scope, harness, &home, &cwd, true)?;
    report("preTool hooks:", vec![line]);
    Ok(())
}

/// Remove one harness hook at `scope`. Leaves unrelated keys in place.
pub fn uninstall_scoped(scope: Scope, harness: &str) -> Result<()> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let line = write_scoped(scope, harness, &home, &cwd, false)?;
    report("preTool hooks:", vec![line]);
    Ok(())
}

pub(crate) fn write_scoped(
    scope: Scope,
    harness: &str,
    home: &Path,
    cwd: &Path,
    install: bool,
) -> Result<String> {
    let agent =
        Agent::from_slug(harness).with_context(|| format!("unknown harness '{harness}'"))?;
    let path = match scope {
        Scope::User => agent.config_path(home),
        Scope::Project => canonical_project_path(agent, cwd),
    };
    let command = hook_command(agent);
    if install {
        write_hook(agent, &path, &command, home)
    } else {
        remove_hook(agent, &path, home, scope)
    }
}

fn write_hook(agent: Agent, path: &Path, command: &str, home: &Path) -> Result<String> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    if agent.hook_uses_command(&existing, command)? {
        return Ok(format!(
            "{}: hook already present in {}",
            agent.name(),
            tilde(path, home)
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let updated = agent.merge(&existing, command)?;
    fs::write(path, updated)?;
    let verb = if agent.has_hook(&existing)? {
        "updated"
    } else {
        "installed"
    };
    Ok(format!(
        "{}: hook {verb} in {}",
        agent.name(),
        tilde(path, home)
    ))
}

fn remove_hook(agent: Agent, path: &Path, home: &Path, scope: Scope) -> Result<String> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let legacy = match scope {
        Scope::User => leftover_json_hook(agent, home)?,
        Scope::Project => None,
    };
    if !agent.has_hook(&existing)? && legacy.is_none() {
        return Ok(format!(
            "{}: hook not present in {}",
            agent.name(),
            tilde(path, home)
        ));
    }
    strip_hook_files(agent, path, &existing, legacy)?;
    Ok(format!(
        "{}: hook removed from {}",
        agent.name(),
        tilde(path, home)
    ))
}

fn strip_hook_files(
    agent: Agent,
    path: &Path,
    existing: &str,
    legacy: Option<(PathBuf, String)>,
) -> Result<()> {
    if matches!(agent, Agent::OpenCode) {
        let _ = fs::remove_file(path);
    } else if agent.has_hook(existing)? {
        fs::write(path, agent.remove(existing)?)?;
    }
    if let Some((legacy_path, content)) = legacy {
        fs::write(&legacy_path, agent.remove(&content)?)?;
    }
    Ok(())
}

/// Remove the `lade hook` interceptor from every agent config that contains it.
pub fn uninstall() -> Result<()> {
    let home = home_dir()?;
    let mut hook_results = Vec::new();
    let mut skill_results = Vec::new();

    for agent in AGENTS {
        let path = agent.config_path(&home);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let legacy = leftover_json_hook(agent, &home)?;
        if agent.has_hook(&existing)? || legacy.is_some() {
            strip_hook_files(agent, &path, &existing, legacy)?;
            hook_results.push(format!(
                "{}: hook removed from {}",
                agent.name(),
                tilde(&path, &home)
            ));
        }
        uninstall_skill(agent, &home, &mut skill_results);
    }

    report("preTool hooks:", hook_results);
    report("skills:", skill_results);
    Ok(())
}

fn uninstall_skill(agent: Agent, home: &Path, results: &mut Vec<String>) {
    let path = agent.skill_path(home);
    let Ok(content) = fs::read_to_string(&path) else {
        return;
    };
    if !is_lade_skill(&content) {
        return;
    }
    let _ = fs::remove_file(&path);
    results.push(format!(
        "{}: skill removed from {}",
        agent.name(),
        tilde(&path, home)
    ));
}

/// Rewrite already-installed hook files to today's command. Never creates
/// a hook the user did not install. Errors are ignored.
///
/// No-op in the unit-test binary: `current_dir()` is the crate and
/// `argv0` is the rustc harness. Refresh still runs in `lade set` /
/// `lade inject` via the real binary. Tests that need a rewrite call
/// `refresh_at` with an isolated home.
pub fn refresh_installed() {
    if cfg!(test) {
        return;
    }
    let Ok(home) = home_dir() else {
        return;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    refresh_at(&home, &cwd);
}

pub(crate) fn refresh_at(home: &Path, cwd: &Path) {
    for agent in AGENTS {
        let command = hook_command(agent);
        refresh_path(agent, &agent.config_path(home), &command);
        if let Ok((project_path, true)) = find_project(agent, home, cwd) {
            refresh_path(agent, &project_path, &command);
        }
        refresh_skill(&agent.skill_path(home));
        if let Ok((path, true)) = find_project_skill(agent, home, cwd) {
            refresh_skill(&path);
        }
    }
    if let Some(path) = find_agents_skill(home, cwd) {
        refresh_skill(&path);
    }
}

fn refresh_skill(path: &Path) {
    let Ok(existing) = fs::read_to_string(path) else {
        return;
    };
    if !is_lade_skill(&existing) || skill_is_current(&existing) {
        return;
    }
    let _ = fs::write(path, SKILL_MD);
}

pub(super) fn refresh_path(agent: Agent, path: &Path, command: &str) {
    let Ok(existing) = fs::read_to_string(path) else {
        return;
    };
    match agent.has_hook(&existing) {
        Ok(true) => {}
        _ => return,
    }
    match agent.hook_uses_command(&existing, command) {
        Ok(true) => return,
        Ok(false) => {}
        Err(_) => return,
    }
    if let Ok(updated) = agent.merge(&existing, command) {
        let _ = fs::write(path, updated);
    }
}
