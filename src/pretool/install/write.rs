use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::message_box::MessageBox;

use super::agent::{AGENTS, Agent};
use super::committed::committed;
use super::locate::{
    canonical_project_path, find_project, hook_state, leftover_json_hook, project_dest,
};
use super::opencode::{OpenCodeApi, is_pristine, plugin};
use super::paths::{ItemVerb, WriteOutcome, home_dir, short_path, tilde};
use super::ui::{PretoolReport, PretoolRow, report, where_line};
use super::want::{Want, want};

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
    report(
        "pre-tool",
        &format!(
            "Wraps commands this harness runs. `lade hook disable --harness {harness}` removes it."
        ),
        vec![line],
    );
    Ok(())
}

/// Remove one harness hook at `scope`. Leaves unrelated keys in place.
pub fn uninstall_scoped(scope: Scope, harness: &str) -> Result<()> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let line = write_scoped(scope, harness, &home, &cwd, false)?;
    report(
        "pre-tool",
        &format!("Removed. `lade hook enable --harness {harness}` puts it back."),
        vec![line],
    );
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
    let dest = match (scope, project_dest(home, cwd)) {
        (Scope::Project, None) => bail!("not a git repo, pre-tool skipped"),
        (_, Some(dest)) => dest,
        (Scope::User, None) => cwd.to_path_buf(),
    };
    let path = hook_path(agent, scope, home, &dest);
    if !install {
        return remove_hook(agent, &path, home, scope, &dest);
    }
    let want = want(agent, scope, &dest)?;
    if project_dest(home, cwd).is_some() {
        refuse_if_other_plane(agent, scope, home, &dest)?;
    }
    if let Some((legacy_path, content)) = leftover_for(agent, home, scope)? {
        fs::write(&legacy_path, agent.remove(&content)?)?;
    }
    let out = write_hook(agent, &path, &want)?;
    let copied = match &want {
        Want::Copy(committed) => {
            format!(", a copy of the committed {}", tilde(&committed.path, home))
        }
        Want::Command(_) => String::new(),
    };
    Ok(format!(
        "{}: hook {} in {}{copied}",
        agent.name(),
        out.verb.label(),
        tilde(&out.path, home)
    ))
}

pub(super) fn write_hook(agent: Agent, path: &Path, want: &Want) -> Result<WriteOutcome> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let current = WriteOutcome {
        verb: ItemVerb::Current,
        path: path.to_path_buf(),
    };
    let updated = match (agent, want) {
        (Agent::OpenCode, Want::Command(command)) => plugin(OpenCodeApi::installed()?, command),
        _ if want.is_current(agent, &existing)? => return Ok(current),
        _ => want.merged(agent, &existing)?,
    };
    if updated.trim_end() == existing.trim_end() {
        return Ok(current);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
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

/// A committed Claude hook is the repo's, never a leftover.
fn leftover_for(agent: Agent, home: &Path, scope: Scope) -> Result<Option<(PathBuf, String)>> {
    match scope {
        Scope::User => leftover_json_hook(agent, home),
        Scope::Project => Ok(None),
    }
}

fn refuse_if_other_plane(agent: Agent, scope: Scope, home: &Path, dest: &Path) -> Result<()> {
    let other = match scope {
        Scope::User => Scope::Project,
        Scope::Project => Scope::User,
    };
    let existing = fs::read_to_string(hook_path(agent, other, home, dest)).unwrap_or_default();
    if !agent.has_hook(&existing).unwrap_or(false) {
        return Ok(());
    }
    refuse_double_plane(&[agent.name()])
}

pub(super) fn refuse_double_plane(names: &[&str]) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    let mut mb = MessageBox::new()
        .error()
        .line("These harnesses have Lade hooks in this repo and under the home directory:");
    for name in names {
        mb = mb.line(format!("- {name}"));
    }
    mb = mb
        .line("Both will run. Remove the home hooks first:")
        .line("`lade hook disable --global --harness <slug>`");
    mb.print_stderr();
    bail!("home and repo hooks both present");
}

fn remove_hook(
    agent: Agent,
    path: &Path,
    home: &Path,
    scope: Scope,
    dest: &Path,
) -> Result<String> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let legacy = leftover_for(agent, home, scope)?;
    let kept = match (scope, agent) {
        (Scope::Project, Agent::Claude) => match committed(dest)? {
            Some(committed) => format!(
                ". The committed hook in {} stays: it is the repo's",
                tilde(&committed.path, home)
            ),
            None => String::new(),
        },
        _ => String::new(),
    };
    if !agent.has_hook(&existing)? && legacy.is_none() {
        return Ok(format!(
            "{}: hook not present in {}{kept}",
            agent.name(),
            tilde(path, home)
        ));
    }
    strip_hook_files(agent, path, &existing, legacy)?;
    Ok(format!(
        "{}: hook removed from {}{kept}",
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
/// Home hooks stay until `lade hook disable --global`.
pub fn teardown() -> Result<PretoolReport> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    teardown_at(&home, &cwd)
}

pub(super) fn teardown_at(home: &Path, cwd: &Path) -> Result<PretoolReport> {
    let Some(dest) = project_dest(home, cwd) else {
        return Ok(PretoolReport {
            where_line: "not a git repo, pre-tool skipped".to_string(),
            rows: Vec::new(),
        });
    };
    uninstall_plane(Scope::Project, home, &dest)
}

pub(super) fn uninstall_plane(scope: Scope, home: &Path, dest: &Path) -> Result<PretoolReport> {
    let mut rows = Vec::new();
    for agent in AGENTS {
        let path = hook_path(agent, scope, home, dest);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let legacy = leftover_for(agent, home, scope)?;
        if agent.has_hook(&existing)? || legacy.is_some() {
            strip_hook_files(agent, &path, &existing, legacy)?;
            rows.push(PretoolRow {
                agent: agent.name(),
                verb: ItemVerb::Removed,
                path: short_path(&path, home, dest),
                note: "",
            });
        }
        if scope == Scope::Project
            && agent == Agent::Claude
            && let Some(committed) = committed(dest)?
        {
            rows.push(PretoolRow {
                agent: agent.name(),
                verb: ItemVerb::Kept,
                path: short_path(&committed.path, home, dest),
                note: "committed, the repo's",
            });
        }
    }
    Ok(PretoolReport {
        where_line: where_line(scope, home, dest),
        rows,
    })
}

/// Rewrite already-installed hook files to today's command. Never creates
/// a hook the user did not install, and never touches an OpenCode plugin
/// edited by hand. Errors are ignored: `health` reports what is left.
pub(crate) fn refresh_at(home: &Path, cwd: &Path, agents: &[Agent]) {
    for &agent in agents {
        let home_path = agent.config_path(home);
        let home_installed = hook_state(&home_path, agent, None).0;
        if let Ok(want) = want(agent, Scope::User, home) {
            refresh_path(agent, &home_path, &want);
        }
        if home_installed {
            continue;
        }
        if let (Ok((project_path, true)), Some(dest)) =
            (find_project(agent, home, cwd), project_dest(home, cwd))
            && let Ok(want) = want(agent, Scope::Project, &dest)
        {
            refresh_path(agent, &project_path, &want);
        }
    }
}

pub(super) fn refresh_path(agent: Agent, path: &Path, want: &Want) {
    let Ok(existing) = fs::read_to_string(path) else {
        return;
    };
    match agent.has_hook(&existing) {
        Ok(true) => {}
        _ => return,
    }
    if matches!(agent, Agent::OpenCode) && !is_pristine(&existing) {
        return;
    }
    match want.is_current(agent, &existing) {
        Ok(true) => return,
        Ok(false) => {}
        Err(_) => return,
    }
    if let Ok(updated) = want.merged(agent, &existing) {
        let _ = fs::write(path, updated);
    }
}
