use std::fs;
use std::path::Path;

use anyhow::Result;

use super::agent::{AGENTS, Agent};
use super::paths::{home_dir, hook_command, tilde};
use super::skill::{SKILL_MD, is_lade_skill, skill_is_current};
use super::ui::{confirm, report};

/// Offer hooks and skills. Empty `only`: detected homes, ask harness then
/// hook then skill. Non-empty `only`: those agents, ask hook and skill.
/// `may_prompt` is true only when stdin and stderr are TTYs.
pub fn install(may_prompt: bool, only: &[&str]) -> Result<()> {
    let home = home_dir()?;
    let mut hook_results = Vec::new();
    let mut skill_results = Vec::new();
    let flagged = !only.is_empty();
    let agents: Vec<Agent> = if flagged {
        only.iter()
            .filter_map(|slug| Agent::from_slug(slug))
            .collect()
    } else {
        AGENTS
            .into_iter()
            .filter(|agent| agent.home_dir(&home).is_dir())
            .collect()
    };

    for agent in agents {
        if !flagged && !offer_harness(agent, &home, may_prompt, &mut hook_results)? {
            continue;
        }
        install_hook(agent, &home, may_prompt, &mut hook_results)?;
        install_skill(agent, &home, may_prompt, &mut skill_results)?;
    }

    report("preTool hooks:", hook_results);
    report("skills:", skill_results);
    Ok(())
}

fn hook_is_current(agent: Agent, home: &Path) -> Result<bool> {
    let existing = fs::read_to_string(agent.config_path(home)).unwrap_or_default();
    agent.hook_uses_command(&existing, &hook_command(agent))
}

fn skill_is_current_at(agent: Agent, home: &Path) -> bool {
    fs::read_to_string(agent.skill_path(home))
        .ok()
        .is_some_and(|content| skill_is_current(&content))
}

fn offer_harness(
    agent: Agent,
    home: &Path,
    may_prompt: bool,
    results: &mut Vec<String>,
) -> Result<bool> {
    if hook_is_current(agent, home)? && skill_is_current_at(agent, home) {
        return Ok(true);
    }
    if !may_prompt {
        results.push(format!(
            "{}: detected. Re-run `lade install` in a terminal",
            agent.name()
        ));
        return Ok(false);
    }
    let path = tilde(&agent.home_dir(home), home);
    if confirm(&format!("Install Lade for {} in {path}?", agent.name()))? {
        Ok(true)
    } else {
        results.push(format!("{}: skipped", agent.name()));
        Ok(false)
    }
}

fn install_hook(
    agent: Agent,
    home: &Path,
    may_prompt: bool,
    results: &mut Vec<String>,
) -> Result<()> {
    let command = hook_command(agent);
    let path = agent.config_path(home);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if agent.has_hook(&existing)? {
        return update_hook(agent, home, may_prompt, results, &path, &existing, &command);
    }
    if !may_prompt {
        results.push(format!(
            "{}: detected. Re-run `lade install` in a terminal to add its hook",
            agent.name()
        ));
        return Ok(());
    }
    if confirm(&format!(
        "Install Lade hook for {} in {}?",
        agent.name(),
        tilde(&path, home)
    ))? {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, agent.merge(&existing, &command)?)?;
        results.push(format!(
            "{}: hook installed in {}",
            agent.name(),
            tilde(&path, home)
        ));
    } else {
        results.push(format!("{}: hook skipped", agent.name()));
    }
    Ok(())
}

fn update_hook(
    agent: Agent,
    home: &Path,
    may_prompt: bool,
    results: &mut Vec<String>,
    path: &Path,
    existing: &str,
    command: &str,
) -> Result<()> {
    if agent.hook_uses_command(existing, command)? {
        results.push(format!("{}: hook already present", agent.name()));
        return Ok(());
    }
    if !may_prompt {
        results.push(format!(
            "{}: detected. Re-run `lade install` in a terminal to update its hook",
            agent.name()
        ));
        return Ok(());
    }
    if confirm(&format!(
        "Update Lade hook for {} in {}?",
        agent.name(),
        tilde(path, home)
    ))? {
        fs::write(path, agent.merge(existing, command)?)?;
        results.push(format!(
            "{}: hook updated in {}",
            agent.name(),
            tilde(path, home)
        ));
    } else {
        results.push(format!("{}: hook update skipped", agent.name()));
    }
    Ok(())
}

fn install_skill(
    agent: Agent,
    home: &Path,
    may_prompt: bool,
    results: &mut Vec<String>,
) -> Result<()> {
    let path = agent.skill_path(home);
    if path.is_file() {
        return update_skill(agent, home, may_prompt, results, &path);
    }
    if !may_prompt {
        results.push(format!(
            "{}: detected. Re-run `lade install` in a terminal to add its skill",
            agent.name()
        ));
        return Ok(());
    }
    if confirm(&format!(
        "Install Lade skill for {} in {}?",
        agent.name(),
        tilde(&path, home)
    ))? {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, SKILL_MD)?;
        results.push(format!(
            "{}: skill installed in {}",
            agent.name(),
            tilde(&path, home)
        ));
    } else {
        results.push(format!("{}: skill skipped", agent.name()));
    }
    Ok(())
}

fn update_skill(
    agent: Agent,
    home: &Path,
    may_prompt: bool,
    results: &mut Vec<String>,
    path: &Path,
) -> Result<()> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    if !is_lade_skill(&existing) {
        results.push(format!(
            "{}: skill present but not Lade-managed",
            agent.name()
        ));
        return Ok(());
    }
    if skill_is_current(&existing) {
        results.push(format!("{}: skill already present", agent.name()));
        return Ok(());
    }
    if !may_prompt {
        results.push(format!(
            "{}: detected. Re-run `lade install` in a terminal to update its skill",
            agent.name()
        ));
        return Ok(());
    }
    if confirm(&format!(
        "Update Lade skill for {} in {}?",
        agent.name(),
        tilde(path, home)
    ))? {
        fs::write(path, SKILL_MD)?;
        results.push(format!(
            "{}: skill updated in {}",
            agent.name(),
            tilde(path, home)
        ));
    } else {
        results.push(format!("{}: skill update skipped", agent.name()));
    }
    Ok(())
}
