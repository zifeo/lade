use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use super::agent::Agent;
use super::locate::{find_project, find_project_skill, hook_state, skill_state};
use super::paths::{home_dir, hook_command, project_hook_command};

#[derive(Debug, Serialize)]
pub struct HookLocation {
    pub path: std::path::PathBuf,
    pub installed: bool,
    pub current: bool,
}

#[derive(Debug, Serialize)]
pub struct PretoolAgentStatus {
    pub global: HookLocation,
    pub project: HookLocation,
}

#[derive(Debug, Serialize)]
pub struct PretoolStatus {
    pub cursor: PretoolAgentStatus,
    pub claude: PretoolAgentStatus,
    pub codex: PretoolAgentStatus,
    pub opencode: PretoolAgentStatus,
}

pub type SkillsStatus = PretoolStatus;

/// Global and project-local Lade-managed skills.
pub fn inspect_skills(cwd: &Path) -> Result<SkillsStatus> {
    let home = home_dir()?;
    Ok(SkillsStatus {
        cursor: inspect_skill_agent(Agent::Cursor, &home, cwd)?,
        claude: inspect_skill_agent(Agent::Claude, &home, cwd)?,
        codex: inspect_skill_agent(Agent::Codex, &home, cwd)?,
        opencode: inspect_skill_agent(Agent::OpenCode, &home, cwd)?,
    })
}

fn inspect_skill_agent(agent: Agent, home: &Path, cwd: &Path) -> Result<PretoolAgentStatus> {
    let global_path = agent.skill_path(home);
    let (global_installed, global_current) = skill_state(&global_path);
    let (project_path, project_installed) = find_project_skill(agent, home, cwd)?;
    let project_current = project_installed && skill_state(&project_path).1;
    Ok(PretoolAgentStatus {
        global: HookLocation {
            path: global_path,
            installed: global_installed,
            current: global_current,
        },
        project: HookLocation {
            path: project_path,
            installed: project_installed,
            current: project_current,
        },
    })
}

/// Global and project-local `lade hook` entries for supported agents.
pub fn inspect(cwd: &Path) -> Result<PretoolStatus> {
    let home = home_dir()?;
    Ok(PretoolStatus {
        cursor: inspect_agent(Agent::Cursor, &home, cwd)?,
        claude: inspect_agent(Agent::Claude, &home, cwd)?,
        codex: inspect_agent(Agent::Codex, &home, cwd)?,
        opencode: inspect_agent(Agent::OpenCode, &home, cwd)?,
    })
}

fn inspect_agent(agent: Agent, home: &Path, cwd: &Path) -> Result<PretoolAgentStatus> {
    let expected = hook_command(agent);
    let global_path = agent.config_path(home);
    let (global_installed, global_current) = hook_state(&global_path, agent, Some(&expected));
    let (project_path, project_installed) = find_project(agent, home, cwd)?;
    let project_expected = project_hook_command(agent);
    let project_current =
        project_installed && hook_state(&project_path, agent, Some(&project_expected)).1;
    Ok(PretoolAgentStatus {
        global: HookLocation {
            path: global_path,
            installed: global_installed,
            current: global_current,
        },
        project: HookLocation {
            path: project_path,
            installed: project_installed,
            current: project_current,
        },
    })
}
