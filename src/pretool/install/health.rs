//! Hooks that need the user. The shell wrap asks for every harness when the
//! hub hands it the hourly slot, and for one harness when the command starts
//! it. `lade status` asks every time.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::agent::{AGENTS, Agent};
use super::locate::find_project;
use super::opencode::OpenCodeApi;
use super::paths::{home_dir, hook_command, project_hook_command, tilde};
use super::write::refresh_at;

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
    for &agent in agents {
        let global = (agent.config_path(home), hook_command(agent), true);
        let project = match find_project(agent, home, cwd) {
            Ok((path, true)) => Some((path, project_hook_command(agent), false)),
            _ => None,
        };
        for (path, command, global) in std::iter::once(global).chain(project) {
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let place = if global { "this machine" } else { "this repo" };
            let fix = match location_line(agent, &content, &command, opencode) {
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

enum Problem {
    Unreadable,
    Rewritable(String),
}

fn location_line(
    agent: Agent,
    content: &str,
    command: &str,
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
    match agent.hook_uses_command(content, command) {
        Ok(true) => None,
        _ => Some(Problem::Rewritable(
            "the Lade hook differs from what this Lade writes.".to_string(),
        )),
    }
}

fn enable_command(agent: Agent, global: bool) -> String {
    let scope = if global { " --global" } else { "" };
    format!("lade hook enable{scope} --harness {}", agent.slug())
}
