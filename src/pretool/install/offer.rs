use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::message_box::MessageBox;

use super::agent::{AGENTS, Agent};
use super::paths::{ItemVerb, home_dir, hook_command, project_hook_command, short_path};
use super::ui::{PretoolReport, PretoolRow, ask_harnesses, where_line};
use super::write::{Scope, hook_path, write_hook};

/// Wrap this repo's harnesses. No git: no pre-tool writes. Never writes home hooks.
pub fn setup(may_prompt: bool, only: &[&str]) -> Result<PretoolReport> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    setup_at(may_prompt, only, &home, &cwd)
}

pub(super) fn setup_at(
    may_prompt: bool,
    only: &[&str],
    home: &Path,
    cwd: &Path,
) -> Result<PretoolReport> {
    let git_root = crate::catalog::git_root(cwd);
    let Some(dest) = git_root else {
        return Ok(PretoolReport {
            where_line: "not a git repo, pre-tool skipped".to_string(),
            rows: Vec::new(),
        });
    };
    let flagged = !only.is_empty();
    let detected = candidates(home, &[]);
    let agents = if flagged {
        candidates(home, only)
    } else if may_prompt {
        let pending = pending_project(&detected, home, &dest);
        if pending.is_empty() {
            detected
        } else {
            ask_harnesses(&pending)?
        }
    } else {
        detected
    };
    refuse_double_plane(&agents, home, &dest)?;
    apply_project(&agents, home, &dest)
}

pub(super) struct Plan {
    pub scope: Scope,
    pub agents: Vec<Agent>,
}

fn candidates(home: &Path, only: &[&str]) -> Vec<Agent> {
    if only.is_empty() {
        return AGENTS
            .into_iter()
            .filter(|agent| agent.home_dir(home).is_dir())
            .collect();
    }
    only.iter()
        .filter_map(|slug| Agent::from_slug(slug))
        .collect()
}

fn apply_project(agents: &[Agent], home: &Path, dest: &Path) -> Result<PretoolReport> {
    let mut rows = Vec::new();
    let mut write = Vec::new();
    for agent in agents {
        if has_hook(*agent, Scope::User, home, dest) {
            rows.push(PretoolRow {
                agent: agent.name(),
                verb: ItemVerb::Flagged,
                path: short_path(&hook_path(*agent, Scope::User, home, dest), home, dest),
                note: "",
            });
            continue;
        }
        write.push(*agent);
    }
    let written = apply_plan(
        &Plan {
            scope: Scope::Project,
            agents: write,
        },
        home,
        dest,
    )?;
    rows.extend(written.rows);
    Ok(PretoolReport {
        where_line: where_line(Scope::Project, home, dest),
        rows,
    })
}

fn refuse_double_plane(agents: &[Agent], home: &Path, dest: &Path) -> Result<()> {
    let stacked: Vec<&str> = agents
        .iter()
        .copied()
        .filter(|agent| {
            has_hook(*agent, Scope::User, home, dest)
                && has_hook(*agent, Scope::Project, home, dest)
        })
        .map(Agent::name)
        .collect();
    if stacked.is_empty() {
        return Ok(());
    }
    let mut mb = MessageBox::new()
        .error()
        .line("These harnesses have Lade hooks in this repo and under the home directory:");
    for name in &stacked {
        mb = mb.line(format!("- {name}"));
    }
    mb = mb
        .line("Both will run. Remove the home hooks first:")
        .line("`lade hook disable --scope user --harness <slug>`");
    mb.print_stderr();
    bail!("home and repo hooks both present");
}

pub(super) fn apply_plan(plan: &Plan, home: &Path, dest: &Path) -> Result<PretoolReport> {
    let mut rows = Vec::new();
    for agent in &plan.agents {
        let out = write_hook(
            *agent,
            &hook_path(*agent, plan.scope, home, dest),
            &hook_command_for(plan.scope, *agent),
        )?;
        rows.push(PretoolRow {
            agent: agent.name(),
            verb: out.verb,
            path: short_path(&out.path, home, dest),
            note: "",
        });
    }
    Ok(PretoolReport {
        where_line: where_line(plan.scope, home, dest),
        rows,
    })
}

fn pending_project(agents: &[Agent], home: &Path, dest: &Path) -> Vec<Agent> {
    agents
        .iter()
        .copied()
        .filter(|agent| hook_needs_write(*agent, Scope::Project, home, dest))
        .collect()
}

fn hook_command_for(scope: Scope, agent: Agent) -> String {
    match scope {
        Scope::User => hook_command(agent),
        Scope::Project => project_hook_command(agent),
    }
}

fn has_hook(agent: Agent, scope: Scope, home: &Path, dest: &Path) -> bool {
    let existing = fs::read_to_string(hook_path(agent, scope, home, dest)).unwrap_or_default();
    agent.has_hook(&existing).unwrap_or(false)
}

fn hook_needs_write(agent: Agent, scope: Scope, home: &Path, dest: &Path) -> bool {
    let existing = fs::read_to_string(hook_path(agent, scope, home, dest)).unwrap_or_default();
    !agent
        .hook_uses_command(&existing, &hook_command_for(scope, agent))
        .unwrap_or(false)
}
