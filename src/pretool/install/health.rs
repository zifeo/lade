//! Hooks that need the user. The shell wrap asks for every harness when the
//! hub hands it the hourly slot, and for one harness when the command starts
//! it. `lade status` asks every time.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::agent::{AGENTS, Agent};
use super::committed::{Committed, committed, committed_path};
use super::locate::{find_project, hook_state, project_dest};
use super::opencode::OpenCodeApi;
use super::paths::{home_dir, hook_command, tilde};
use super::want::{Want, want};
use super::write::{Scope, refresh_at};

/// Every harness. Rewrites what Lade wrote, then lists what is left for the
/// user.
pub fn refresh_and_report() -> Result<Vec<String>> {
    refresh_and_report_for(&AGENTS)
}

/// Only the harness `command` starts. Nothing for any other command.
pub fn refresh_and_report_launch(command: &str) -> Result<Vec<String>> {
    match Agent::launched_by(command) {
        Some(agent) => refresh_and_report_for(&[agent]),
        None => Ok(Vec::new()),
    }
}

fn refresh_and_report_for(agents: &[Agent]) -> Result<Vec<String>> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    refresh_at(&home, &cwd, agents);
    Ok(report_at(&home, &cwd, agents, &installed_opencode))
}

/// Read only.
pub fn report() -> Result<Vec<String>> {
    let home = home_dir()?;
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    Ok(report_at(&home, &cwd, &AGENTS, &installed_opencode))
}

// `opencode --version` takes up to 0.6s, so only when a Lade plugin is there.
fn installed_opencode() -> Option<OpenCodeApi> {
    OpenCodeApi::installed().ok()
}

/// `opencode`: the API of the `opencode` on PATH, when there is one. Only
/// called for an OpenCode Lade plugin.
pub(crate) fn report_at(
    home: &Path,
    cwd: &Path,
    agents: &[Agent],
    opencode: &dyn Fn() -> Option<OpenCodeApi>,
) -> Vec<String> {
    let mut lines = Vec::new();
    let dest = project_dest(home, cwd);
    for &agent in agents {
        let committed = match (agent, &dest) {
            (Agent::Claude, Some(dest)) => committed_or_line(dest, home, &mut lines),
            _ => None,
        };
        let global = (
            agent.config_path(home),
            Want::Command(hook_command(agent)),
            true,
        );
        let project = match (find_project(agent, home, cwd), &dest) {
            (Ok((path, true)), Some(dest)) => match want(agent, Scope::Project, dest) {
                Ok(want) => Some((path, want, false)),
                Err(_) => None,
            },
            _ => None,
        };
        let home_has = hook_state(&agent.config_path(home), agent, None).0;
        if home_has && (project.is_some() || committed.is_some()) {
            lines.push(format!(
                "{}: hooks in this repo and under the home directory. Both will run. Remove the home hooks first: `lade hook disable --global --harness {}`",
                agent.name(),
                agent.slug()
            ));
        }
        if let (Some(committed), None, false) = (&committed, &project, home_has) {
            lines.push(format!(
                "Claude Code (this repo, {}): no copy of the committed hook, so a session started in a subdirectory runs commands without Lade. `lade hook enable --harness claude` writes it.",
                tilde(
                    &committed.path.with_file_name("settings.local.json"),
                    home
                )
            ));
        }
        for (path, want, global) in std::iter::once(global).chain(project) {
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let place = if global { "this machine" } else { "this repo" };
            let fix = match location_line(agent, &content, &want, opencode) {
                None => continue,
                Some(Problem::Unreadable) => {
                    "not valid JSON. Fix it by hand: Lade does not overwrite it.".to_string()
                }
                Some(Problem::Rewritable(line)) => {
                    format!("{line} `{}` rewrites it.", enable_command(agent, global))
                }
            };
            lines.push(format!(
                "{} ({place}, {}): {fix}",
                agent.name(),
                tilde(&path, home)
            ));
        }
    }
    lines
}

/// The repo's committed Claude hook. An unreadable file is a line, not a hook.
fn committed_or_line(dest: &Path, home: &Path, lines: &mut Vec<String>) -> Option<Committed> {
    match committed(dest) {
        Ok(found) => found,
        Err(_) => {
            lines.push(format!(
                "Claude Code (this repo, {}): not valid JSON. Fix it by hand: Lade does not overwrite it.",
                tilde(&committed_path(dest), home)
            ));
            None
        }
    }
}

enum Problem {
    Unreadable,
    Rewritable(String),
}

fn location_line(
    agent: Agent,
    content: &str,
    want: &Want,
    opencode: &dyn Fn() -> Option<OpenCodeApi>,
) -> Option<Problem> {
    match agent.has_hook(content) {
        Err(_) => return Some(Problem::Unreadable),
        Ok(false) => return None,
        Ok(true) => {}
    }
    if let (Agent::OpenCode, Some(file)) = (agent, OpenCodeApi::of(content))
        && let Some(installed) = opencode()
        && file != installed
    {
        return Some(Problem::Rewritable(format!(
            "the plugin uses the OpenCode {} API, `opencode` is {}.",
            file.label(),
            installed.label()
        )));
    }
    match (want.is_current(agent, content), want) {
        (Ok(true), _) => None,
        (_, Want::Copy(_)) => Some(Problem::Rewritable(
            "the Lade hook differs from the committed one in .claude/settings.json, so a session started at the repo root runs this copy.".to_string(),
        )),
        (_, Want::Command(_)) => Some(Problem::Rewritable(
            "the Lade hook differs from what this Lade writes.".to_string(),
        )),
    }
}

fn enable_command(agent: Agent, global: bool) -> String {
    let scope = if global { " --global" } else { "" };
    format!("lade hook enable{scope} --harness {}", agent.slug())
}
