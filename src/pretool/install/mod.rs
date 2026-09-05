//! Optional installation of the `lade hook` interceptor into the agents that
//! support `preToolUse` shell hooks (Cursor, Claude Code, Codex, Pi, OpenCode).
//!
//! `lade install` is a global, once-only operation, so these hooks are written
//! to the agents' global config (`~/.cursor/hooks.json`,
//! `~/.claude/settings.json`, `~/.codex/hooks.json`, `~/.pi/agent/settings.json`,
//! `~/.config/opencode/plugins/lade-pretool.js`). We only act when the agent's
//! home dir already exists and never overwrite unrelated settings.

mod config;
#[cfg(test)]
mod tests;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use config::AGENTS;
use serde::Serialize;

use crate::message_box::MessageBox;

fn home_dir() -> Result<PathBuf> {
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("cannot determine home directory")
}

fn hook_command() -> String {
    format!("{} hook", crate::pretool::invoked_lade_bin())
}

fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn confirm(name: &str, path: &str) -> Result<bool> {
    eprint!("Install Lade hook for {name} in {path}? [y/N]: ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let answer = answer.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn report(results: Vec<String>) {
    if results.is_empty() {
        return;
    }
    let mut mb = MessageBox::new().info().line("preTool hooks:");
    for result in results {
        mb = mb.line(format!("- {result}"));
    }
    mb.print_plain_stderr();
}

/// Offer to install the `lade hook` interceptor for every agent detected on the
/// machine. `may_prompt` must be true only when both stdin and stderr are TTYs.
pub fn install(may_prompt: bool) -> Result<()> {
    let command = hook_command();
    let home = home_dir()?;
    let mut results = Vec::new();

    for agent in AGENTS {
        if !agent.home_dir(&home).is_dir() {
            continue;
        }
        let path = agent.config_path(&home);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        if agent.has_hook(&existing)? {
            if agent.hook_uses_command(&existing, &command)? {
                results.push(format!("{}: hook already present", agent.name()));
                continue;
            }
            if !may_prompt {
                results.push(format!(
                    "{}: detected — re-run `lade install` in a terminal to update its hook",
                    agent.name()
                ));
                continue;
            }
            fs::write(&path, agent.merge(&existing, &command)?)?;
            results.push(format!(
                "{}: hook updated in {}",
                agent.name(),
                tilde(&path, &home)
            ));
            continue;
        }
        if !may_prompt {
            results.push(format!(
                "{}: detected — re-run `lade install` in a terminal to add its hook",
                agent.name()
            ));
            continue;
        }
        if confirm(agent.name(), &tilde(&path, &home))? {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, agent.merge(&existing, &command)?)?;
            results.push(format!(
                "{}: hook installed in {}",
                agent.name(),
                tilde(&path, &home)
            ));
        } else {
            results.push(format!("{}: skipped", agent.name()));
        }
    }

    report(results);
    Ok(())
}

/// Remove the `lade hook` interceptor from every agent config that contains it.
pub fn uninstall() -> Result<()> {
    let home = home_dir()?;
    let mut results = Vec::new();

    for agent in AGENTS {
        let path = agent.config_path(&home);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let legacy = leftover_json_hook(agent, &home)?;
        if !agent.has_hook(&existing)? && legacy.is_none() {
            continue;
        }
        if matches!(agent, config::Agent::OpenCode) {
            let _ = fs::remove_file(&path);
        } else if agent.has_hook(&existing)? {
            fs::write(&path, agent.remove(&existing)?)?;
        }
        if let Some((legacy_path, content)) = legacy {
            fs::write(&legacy_path, agent.remove(&content)?)?;
        }
        results.push(format!(
            "{}: hook removed from {}",
            agent.name(),
            tilde(&path, &home)
        ));
    }

    report(results);
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct HookLocation {
    pub path: PathBuf,
    pub installed: bool,
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
    pub pi: PretoolAgentStatus,
    pub opencode: PretoolAgentStatus,
}

/// Global and project-local `lade hook` entries for supported agents.
pub fn inspect(cwd: &Path) -> Result<PretoolStatus> {
    let home = home_dir()?;
    Ok(PretoolStatus {
        cursor: inspect_agent(config::Agent::Cursor, &home, cwd)?,
        claude: inspect_agent(config::Agent::Claude, &home, cwd)?,
        codex: inspect_agent(config::Agent::Codex, &home, cwd)?,
        pi: inspect_agent(config::Agent::Pi, &home, cwd)?,
        opencode: inspect_agent(config::Agent::OpenCode, &home, cwd)?,
    })
}

fn inspect_agent(agent: config::Agent, home: &Path, cwd: &Path) -> Result<PretoolAgentStatus> {
    let global_path = agent.config_path(home);
    let global_installed = hook_present(&global_path, agent)?;
    let (project_path, project_installed) = find_project(agent, home, cwd)?;
    Ok(PretoolAgentStatus {
        global: HookLocation {
            path: global_path,
            installed: global_installed,
        },
        project: HookLocation {
            path: project_path,
            installed: project_installed,
        },
    })
}

fn hook_present(path: &Path, agent: config::Agent) -> Result<bool> {
    match fs::read_to_string(path) {
        Ok(content) => agent.has_hook(&content),
        Err(_) => Ok(false),
    }
}

fn leftover_json_hook(agent: config::Agent, home: &Path) -> Result<Option<(PathBuf, String)>> {
    let Some(path) = agent.legacy_json_path(home) else {
        return Ok(None);
    };
    match fs::read_to_string(&path) {
        Ok(content) if config::Agent::Claude.has_hook(&content)? => Ok(Some((path, content))),
        _ => Ok(None),
    }
}

/// Walk from `cwd` toward `$HOME` looking for a project-local hook file.
/// Home-level agent configs stay in `global`.
fn find_project(agent: config::Agent, home: &Path, cwd: &Path) -> Result<(PathBuf, bool)> {
    let mut dir = cwd.to_path_buf();
    loop {
        if dir == home {
            break;
        }
        let files = project_files(agent, &dir);
        for path in &files {
            if path.is_file() && hook_present(path, agent)? {
                return Ok((path.clone(), true));
            }
        }
        if let Some(found) = files.into_iter().find(|path| path.is_file()) {
            return Ok((found, false));
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => break,
        }
    }
    Ok((canonical_project_path(agent, cwd), false))
}

fn project_files(agent: config::Agent, dir: &Path) -> Vec<PathBuf> {
    match agent {
        config::Agent::Cursor => vec![dir.join(".cursor").join("hooks.json")],
        config::Agent::Claude => vec![
            dir.join(".claude").join("settings.local.json"),
            dir.join(".claude").join("settings.json"),
        ],
        config::Agent::Codex => vec![dir.join(".codex").join("hooks.json")],
        config::Agent::Pi => vec![
            dir.join(".pi").join("settings.json"),
            dir.join(".pi").join("hooks.json"),
        ],
        config::Agent::OpenCode => vec![
            dir.join(".opencode")
                .join("plugins")
                .join("lade-pretool.js"),
        ],
    }
}

fn canonical_project_path(agent: config::Agent, cwd: &Path) -> PathBuf {
    match agent {
        config::Agent::Cursor => cwd.join(".cursor").join("hooks.json"),
        config::Agent::Claude => cwd.join(".claude").join("settings.json"),
        config::Agent::Codex => cwd.join(".codex").join("hooks.json"),
        config::Agent::Pi => cwd.join(".pi").join("settings.json"),
        config::Agent::OpenCode => cwd
            .join(".opencode")
            .join("plugins")
            .join("lade-pretool.js"),
    }
}
