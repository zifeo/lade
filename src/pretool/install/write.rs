use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::agent::{AGENTS, Agent};
use super::locate::{canonical_project_path, find_project, leftover_json_hook};
use super::paths::{
    ItemVerb, WriteOutcome, home_dir, hook_command, project_hook_command, short_path, tilde,
};
use super::ui::{PretoolReport, PretoolRow, report, where_line};

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
    report("pre-tool:", vec![line]);
    Ok(())
}

/// Remove one harness hook at `scope`. Leaves unrelated keys in place.
pub fn uninstall_scoped(scope: Scope, harness: &str) -> Result<()> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let line = write_scoped(scope, harness, &home, &cwd, false)?;
    report("pre-tool:", vec![line]);
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
    let command = match scope {
        Scope::User => hook_command(agent),
        Scope::Project => project_hook_command(agent),
    };
    if install {
        let out = write_hook(agent, &path, &command)?;
        Ok(format!(
            "{}: hook {} in {}",
            agent.name(),
            out.verb.label(),
            tilde(&out.path, home)
        ))
    } else {
        remove_hook(agent, &path, home, scope)
    }
}

pub(super) fn write_hook(agent: Agent, path: &Path, command: &str) -> Result<WriteOutcome> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    if agent.hook_uses_command(&existing, command)? {
        return Ok(WriteOutcome {
            verb: ItemVerb::Current,
            path: path.to_path_buf(),
        });
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let updated = agent.merge(&existing, command)?;
    fs::write(path, updated)?;
    let verb = if agent.has_hook(&existing)? {
        ItemVerb::Updated
    } else {
        ItemVerb::Installed
    };
    Ok(WriteOutcome {
        verb,
        path: path.to_path_buf(),
    })
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

pub(super) fn hook_path(agent: Agent, scope: Scope, home: &Path, dest: &Path) -> PathBuf {
    match scope {
        Scope::User => agent.config_path(home),
        Scope::Project => canonical_project_path(agent, dest),
    }
}

/// Remove Lade project hooks in this repo. No git: no pre-tool writes.
/// Home hooks stay until `lade hook disable --scope user`.
pub fn teardown() -> Result<PretoolReport> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let git_root = crate::catalog::git_root(&cwd);
    let Some(dest) = git_root else {
        return Ok(PretoolReport {
            where_line: "not a git repo, pre-tool skipped".to_string(),
            rows: Vec::new(),
        });
    };
    uninstall_plane(Scope::Project, &home, &dest)
}

pub(super) fn uninstall_plane(scope: Scope, home: &Path, dest: &Path) -> Result<PretoolReport> {
    let mut rows = Vec::new();
    for agent in AGENTS {
        let path = hook_path(agent, scope, home, dest);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let legacy = match scope {
            Scope::User => leftover_json_hook(agent, home)?,
            Scope::Project => None,
        };
        if agent.has_hook(&existing)? || legacy.is_some() {
            strip_hook_files(agent, &path, &existing, legacy)?;
            rows.push(PretoolRow {
                agent: agent.name(),
                verb: ItemVerb::Removed,
                path: short_path(&path, home, dest),
                note: "",
            });
        }
    }
    Ok(PretoolReport {
        where_line: where_line(scope, home, dest),
        rows,
    })
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
        refresh_path(agent, &agent.config_path(home), &hook_command(agent));
        if let Ok((project_path, true)) = find_project(agent, home, cwd) {
            refresh_path(agent, &project_path, &project_hook_command(agent));
        }
    }
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
