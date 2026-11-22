use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use super::agent::Agent;
use super::committed::{committed, committed_path};
use super::locate::{find_project, hook_state, project_dest, unreadable};
use super::paths::{home_dir, hook_command};
use super::want::{Want, want};
use super::write::Scope;

#[derive(Debug, Serialize)]
pub struct HookLocation {
    pub path: std::path::PathBuf,
    pub installed: bool,
    pub current: bool,
    /// The file and why Lade cannot read a hook from it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The Lade hook in the repo's committed `.claude/settings.json`.
#[derive(Debug, Serialize)]
pub struct CommittedHook {
    // § dpgrarwms.e@wg9xq
    pub path: std::path::PathBuf,
    pub present: bool,
    /// `"onFailure": "block"`: a missing or failing Lade blocks the command.
    pub blocking: bool,
    /// The file and why Lade cannot read it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PretoolAgentStatus {
    // § one fmn2jmqxb.c@ey3cc
    pub global: HookLocation,
    /// `current`: the snapshot, or a copy of the committed hook when there is one.
    pub project: HookLocation,
    /// Claude Code only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed: Option<CommittedHook>,
}

#[derive(Debug, Serialize)]
pub struct PretoolStatus {
    // § khcwz2ec3.d@6qm0b
    pub cursor: PretoolAgentStatus,
    pub claude: PretoolAgentStatus,
    pub codex: PretoolAgentStatus,
    pub opencode: PretoolAgentStatus,
}

/// Global and project-local `lade hook` entries for supported agents.
pub fn inspect(cwd: &Path) -> Result<PretoolStatus> {
    inspect_at(&home_dir()?, cwd)
}

pub(super) fn inspect_at(home: &Path, cwd: &Path) -> Result<PretoolStatus> {
    Ok(PretoolStatus {
        cursor: inspect_agent(Agent::Cursor, home, cwd)?,
        claude: inspect_agent(Agent::Claude, home, cwd)?,
        codex: inspect_agent(Agent::Codex, home, cwd)?,
        opencode: inspect_agent(Agent::OpenCode, home, cwd)?,
    })
}

fn inspect_agent(agent: Agent, home: &Path, cwd: &Path) -> Result<PretoolAgentStatus> {
    // § khcwz2ec3.d@wcwwm
    let expected = hook_command(agent);
    let global_path = agent.config_path(home);
    let (global_installed, global_current) = hook_state(
        &global_path,
        agent,
        Some(&Want::Command(expected, Scope::User)),
    );
    let global_error = unreadable(&global_path, agent);
    let (project_path, project_installed) = find_project(agent, home, cwd)?;
    let plane = project_dest(home, cwd);
    let project_error = plane
        .as_ref()
        .and_then(|_| unreadable(&project_path, agent));
    let dest = plane.unwrap_or_else(|| cwd.to_path_buf());
    let project_current = project_installed
        && match want(agent, Scope::Project, &dest) {
            Ok(want) => std::fs::read_to_string(&project_path)
                .is_ok_and(|content| want.is_current(agent, &content).unwrap_or(false)),
            Err(_) => false,
        };
    let committed = (agent == Agent::Claude).then(|| {
        let (found, error) = match committed(&dest) {
            Ok(found) => (found, None),
            Err(err) => (None, Some(format!("{err:#}"))),
        };
        CommittedHook {
            path: committed_path(&dest),
            present: found.is_some(),
            blocking: found.as_ref().is_some_and(|found| found.blocking()),
            error,
        }
    });
    Ok(PretoolAgentStatus {
        global: HookLocation {
            path: global_path,
            installed: global_installed,
            current: global_current,
            error: global_error,
        },
        project: HookLocation {
            path: project_path,
            installed: project_installed,
            current: project_current,
            error: project_error,
        },
        committed,
    })
}
