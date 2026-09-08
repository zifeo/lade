use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::agent::{AGENTS, Agent};
use super::paths::{ItemVerb, home_dir, hook_command, project_hook_command, short_path};
use super::skill::{is_lade_skill, skill_is_current, write_skill};
use super::ui::{
    PretoolReport, PretoolRow, ask_agents, ask_repo_or_machine, default_scope, warn_double_hooks,
    where_line,
};
use super::write::{Scope, hook_path, skill_path, write_hook};

/// Offer hooks and skills on one plane. Git cwd defaults to the repo
/// (hooks and skills together). No git defaults to this machine.
/// A complete default plane skips prompts. Clap has no multi-select.
pub fn install(may_prompt: bool, only: &[&str]) -> Result<PretoolReport> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let git_root = crate::catalog::git_root(&cwd);
    let dest = git_root.as_deref().unwrap_or(cwd.as_path());
    let in_git = git_root.is_some();
    let flagged = !only.is_empty();
    let candidates = candidates(&home, only);
    if may_prompt {
        interactive(&candidates, flagged, in_git, &home, dest)
    } else {
        announce_noninteractive(&candidates, default_scope(in_git), &home, dest)
    }
}

pub(super) struct Plan {
    pub scope: Scope,
    pub hooks: bool,
    pub skills: bool,
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

fn interactive(
    candidates: &[Agent],
    flagged: bool,
    in_git: bool,
    home: &Path,
    dest: &Path,
) -> Result<PretoolReport> {
    let default = default_scope(in_git);
    if !candidates.is_empty() && pending(candidates, default, true, true, home, dest).is_empty() {
        return apply_plan(
            &Plan {
                scope: default,
                hooks: true,
                skills: true,
                agents: candidates.to_vec(),
            },
            home,
            dest,
        );
    }
    let scope = choose_scope(in_git, candidates, home, dest)?;
    let agents = if flagged {
        candidates.to_vec()
    } else {
        let pending = pending(candidates, scope, true, true, home, dest);
        if pending.is_empty() && !candidates.is_empty() {
            candidates.to_vec()
        } else {
            ask_agents(&pending)?
        }
    };
    if scope == Scope::Project {
        let stacked = agents
            .iter()
            .copied()
            .filter(|agent| has_hook(*agent, Scope::User, home, dest))
            .map(Agent::name)
            .map(str::to_string)
            .collect::<Vec<_>>();
        warn_double_hooks(&stacked);
    }
    apply_plan(
        &Plan {
            scope,
            hooks: true,
            skills: true,
            agents,
        },
        home,
        dest,
    )
}

fn choose_scope(in_git: bool, candidates: &[Agent], home: &Path, dest: &Path) -> Result<Scope> {
    if !in_git {
        return Ok(Scope::User);
    }
    if candidates
        .iter()
        .any(|agent| has_hook(*agent, Scope::Project, home, dest))
    {
        return Ok(Scope::Project);
    }
    ask_repo_or_machine()
}

fn announce_noninteractive(
    candidates: &[Agent],
    scope: Scope,
    home: &Path,
    dest: &Path,
) -> Result<PretoolReport> {
    let mut rows = Vec::new();
    for agent in candidates {
        rows.extend(peek_agent(*agent, scope, home, dest)?);
    }
    Ok(PretoolReport {
        where_line: where_line(scope, home, dest),
        rows,
    })
}

pub(super) fn apply_plan(plan: &Plan, home: &Path, dest: &Path) -> Result<PretoolReport> {
    let mut rows = Vec::new();
    for agent in &plan.agents {
        if plan.hooks {
            let out = write_hook(
                *agent,
                &hook_path(*agent, plan.scope, home, dest),
                &hook_command_for(plan.scope, *agent),
            )?;
            rows.push(PretoolRow {
                agent: agent.name(),
                verb: out.verb,
                path: short_path(&out.path, home, dest),
            });
        }
        if plan.skills {
            let out = write_skill(*agent, &skill_path(*agent, plan.scope, home, dest))?;
            rows.push(PretoolRow {
                agent: agent.name(),
                verb: out.verb,
                path: short_path(&out.path, home, dest),
            });
        }
    }
    Ok(PretoolReport {
        where_line: where_line(plan.scope, home, dest),
        rows,
    })
}

fn pending(
    agents: &[Agent],
    scope: Scope,
    hooks: bool,
    skills: bool,
    home: &Path,
    dest: &Path,
) -> Vec<Agent> {
    agents
        .iter()
        .copied()
        .filter(|agent| {
            (hooks && hook_needs_write(*agent, scope, home, dest))
                || (skills && skill_needs_write(*agent, scope, home, dest))
        })
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

fn skill_needs_write(agent: Agent, scope: Scope, home: &Path, dest: &Path) -> bool {
    match fs::read_to_string(skill_path(agent, scope, home, dest)) {
        Ok(content) if is_lade_skill(&content) => !skill_is_current(&content),
        Ok(_) => false,
        Err(_) => true,
    }
}

fn peek_agent(agent: Agent, scope: Scope, home: &Path, dest: &Path) -> Result<Vec<PretoolRow>> {
    let hook_file = hook_path(agent, scope, home, dest);
    let existing = fs::read_to_string(&hook_file).unwrap_or_default();
    let command = hook_command_for(scope, agent);
    let hook_verb = if agent.hook_uses_command(&existing, &command)? {
        ItemVerb::Current
    } else if agent.has_hook(&existing)? {
        ItemVerb::Stale
    } else {
        ItemVerb::Missing
    };
    let skill_file = skill_path(agent, scope, home, dest);
    let skill_verb = match fs::read_to_string(&skill_file) {
        Ok(content) if !is_lade_skill(&content) => ItemVerb::Unmanaged,
        Ok(content) if skill_is_current(&content) => ItemVerb::Current,
        Ok(_) => ItemVerb::Stale,
        Err(_) => ItemVerb::Missing,
    };
    Ok(vec![
        PretoolRow {
            agent: agent.name(),
            verb: hook_verb,
            path: short_path(&hook_file, home, dest),
        },
        PretoolRow {
            agent: agent.name(),
            verb: skill_verb,
            path: short_path(&skill_file, home, dest),
        },
    ])
}
