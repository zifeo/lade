//! Optional installation of the `lade hook` interceptor into the agents that
//! support `preToolUse` shell hooks (Cursor, Claude Code, Codex, OpenCode).
//!
//! `lade install` is a global, once-only operation, so these hooks are written
//! to the agents' global config (`~/.cursor/hooks.json`,
//! `~/.claude/settings.json`, `~/.codex/hooks.json`,
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
use sha2::{Digest, Sha256};

use crate::message_box::MessageBox;

fn home_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    if let Some(home) = std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("cannot determine home directory")
}

fn install_bin() -> String {
    let bin = crate::pretool::invoked_lade_bin();
    match Path::new(&bin).file_name().and_then(|name| name.to_str()) {
        Some("lade" | "lade.exe") => bin,
        _ => "lade".to_string(),
    }
}

fn hook_command(agent: config::Agent) -> String {
    format!("{} hook --harness {}", install_bin(), agent.slug())
}

pub(super) const SKILL_MD: &str = include_str!("../../../.agents/skills/lade/SKILL.md");

/// sha256 of official SKILL.md blobs this binary still rewrites.
const SKILL_PREVIOUS: &[&str] = &[
    "1e8636e98007c49b9b2cb78f716c67e714fbda2bfdcd6070c0abda9b436a914a",
    "dee189b102053fbc2a672bcfa6ce7a6b6e4c483ba68ee71244848d60f35fedca",
];

fn skill_sha256(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

pub(super) fn is_lade_skill(content: &str) -> bool {
    skill_is_current(content)
        || SKILL_PREVIOUS
            .iter()
            .any(|known| *known == skill_sha256(content))
        || test_previous_official(content)
}

#[cfg(test)]
const STALE_OFFICIAL: &str =
    "---\nname: lade\ndescription: previous official lade skill\n---\n\n# Lade\n";

#[cfg(test)]
fn test_previous_official(content: &str) -> bool {
    content == STALE_OFFICIAL
}

#[cfg(not(test))]
fn test_previous_official(_: &str) -> bool {
    false
}

pub(super) fn skill_is_current(content: &str) -> bool {
    content == SKILL_MD
}

fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn confirm(kind: &str, name: &str, path: &str) -> Result<bool> {
    eprint!("Install Lade {kind} for {name} in {path}? [y/N]: ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let answer = answer.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn report(title: &str, results: Vec<String>) {
    if results.is_empty() {
        return;
    }
    let mut mb = MessageBox::new().info().line(title);
    for result in results {
        mb = mb.line(format!("- {result}"));
    }
    mb.print_plain_stderr();
}

/// Offer to install the `lade hook` interceptor for every agent detected on the
/// machine. `may_prompt` must be true only when both stdin and stderr are TTYs.
pub fn install(may_prompt: bool) -> Result<()> {
    let home = home_dir()?;
    let mut hook_results = Vec::new();
    let mut skill_results = Vec::new();

    for agent in AGENTS {
        if !agent.home_dir(&home).is_dir() {
            continue;
        }
        install_hook(agent, &home, may_prompt, &mut hook_results)?;
        install_skill(agent, &home, may_prompt, &mut skill_results)?;
    }

    report("preTool hooks:", hook_results);
    report("skills:", skill_results);
    Ok(())
}

fn install_hook(
    agent: config::Agent,
    home: &Path,
    may_prompt: bool,
    results: &mut Vec<String>,
) -> Result<()> {
    let command = hook_command(agent);
    let path = agent.config_path(home);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if agent.has_hook(&existing)? {
        if agent.hook_uses_command_scoped(&existing, &command, false)? {
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
        fs::write(&path, agent.merge(&existing, &command)?)?;
        results.push(format!(
            "{}: hook updated in {}",
            agent.name(),
            tilde(&path, home)
        ));
        return Ok(());
    }
    if !may_prompt {
        results.push(format!(
            "{}: detected. Re-run `lade install` in a terminal to add its hook",
            agent.name()
        ));
        return Ok(());
    }
    if confirm("hook", agent.name(), &tilde(&path, home))? {
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

fn install_skill(
    agent: config::Agent,
    home: &Path,
    may_prompt: bool,
    results: &mut Vec<String>,
) -> Result<()> {
    let path = agent.skill_path(home);
    if path.is_file() {
        let existing = fs::read_to_string(&path).unwrap_or_default();
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
        fs::write(&path, SKILL_MD)?;
        results.push(format!(
            "{}: skill updated in {}",
            agent.name(),
            tilde(&path, home)
        ));
        return Ok(());
    }
    if !may_prompt {
        results.push(format!(
            "{}: detected. Re-run `lade install` in a terminal to add its skill",
            agent.name()
        ));
        return Ok(());
    }
    if confirm("skill", agent.name(), &tilde(&path, home))? {
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
        let command = hook_command(agent);
        refresh_path(agent, &agent.config_path(home), &command, false);
        if let Ok((project_path, true)) = find_project(agent, home, cwd) {
            refresh_path(agent, &project_path, &command, true);
        }
        refresh_skill(&agent.skill_path(home));
        if let Ok((path, true)) = find_project_skill(agent, home, cwd) {
            refresh_skill(&path);
        }
    }
    if let Some(path) = find_agents_skill(home, cwd) {
        refresh_skill(&path);
    }
}

fn refresh_skill(path: &Path) {
    let Ok(existing) = fs::read_to_string(path) else {
        return;
    };
    if !is_lade_skill(&existing) || skill_is_current(&existing) {
        return;
    }
    let _ = fs::write(path, SKILL_MD);
}

fn refresh_path(agent: config::Agent, path: &Path, command: &str, project: bool) {
    let Ok(existing) = fs::read_to_string(path) else {
        return;
    };
    match agent.has_hook(&existing) {
        Ok(true) => {}
        _ => return,
    }
    match agent.hook_uses_command_scoped(&existing, command, project) {
        Ok(true) => return,
        Ok(false) => {}
        Err(_) => return,
    }
    if let Ok(updated) = agent.merge_scoped(&existing, command, project) {
        let _ = fs::write(path, updated);
    }
}

/// Remove the `lade hook` interceptor from every agent config that contains it.
pub fn uninstall() -> Result<()> {
    let home = home_dir()?;
    let mut hook_results = Vec::new();
    let mut skill_results = Vec::new();

    for agent in AGENTS {
        let path = agent.config_path(&home);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let legacy = leftover_json_hook(agent, &home)?;
        if agent.has_hook(&existing)? || legacy.is_some() {
            if matches!(agent, config::Agent::OpenCode) {
                let _ = fs::remove_file(&path);
            } else if agent.has_hook(&existing)? {
                fs::write(&path, agent.remove(&existing)?)?;
            }
            if let Some((legacy_path, content)) = legacy {
                fs::write(&legacy_path, agent.remove(&content)?)?;
            }
            hook_results.push(format!(
                "{}: hook removed from {}",
                agent.name(),
                tilde(&path, &home)
            ));
        }
        uninstall_skill(agent, &home, &mut skill_results);
    }

    report("preTool hooks:", hook_results);
    report("skills:", skill_results);
    Ok(())
}

fn uninstall_skill(agent: config::Agent, home: &Path, results: &mut Vec<String>) {
    let path = agent.skill_path(home);
    let Ok(content) = fs::read_to_string(&path) else {
        return;
    };
    if !is_lade_skill(&content) {
        return;
    }
    let _ = fs::remove_file(&path);
    results.push(format!(
        "{}: skill removed from {}",
        agent.name(),
        tilde(&path, home)
    ));
}

#[derive(Debug, Serialize)]
pub struct HookLocation {
    pub path: PathBuf,
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
        cursor: inspect_skill_agent(config::Agent::Cursor, &home, cwd)?,
        claude: inspect_skill_agent(config::Agent::Claude, &home, cwd)?,
        codex: inspect_skill_agent(config::Agent::Codex, &home, cwd)?,
        opencode: inspect_skill_agent(config::Agent::OpenCode, &home, cwd)?,
    })
}

fn inspect_skill_agent(
    agent: config::Agent,
    home: &Path,
    cwd: &Path,
) -> Result<PretoolAgentStatus> {
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

fn skill_state(path: &Path) -> (bool, bool) {
    let Ok(content) = fs::read_to_string(path) else {
        return (false, false);
    };
    let installed = is_lade_skill(&content);
    (installed, installed && skill_is_current(&content))
}

/// Global and project-local `lade hook` entries for supported agents.
pub fn inspect(cwd: &Path) -> Result<PretoolStatus> {
    let home = home_dir()?;
    Ok(PretoolStatus {
        cursor: inspect_agent(config::Agent::Cursor, &home, cwd)?,
        claude: inspect_agent(config::Agent::Claude, &home, cwd)?,
        codex: inspect_agent(config::Agent::Codex, &home, cwd)?,
        opencode: inspect_agent(config::Agent::OpenCode, &home, cwd)?,
    })
}

fn inspect_agent(agent: config::Agent, home: &Path, cwd: &Path) -> Result<PretoolAgentStatus> {
    let expected = hook_command(agent);
    let global_path = agent.config_path(home);
    let (global_installed, global_current) = hook_state(&global_path, agent, Some(&expected));
    let (project_path, project_installed) = find_project(agent, home, cwd)?;
    let project_current = project_installed && hook_state(&project_path, agent, Some(&expected)).1;
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

fn hook_state(path: &Path, agent: config::Agent, expected: Option<&str>) -> (bool, bool) {
    let Ok(content) = fs::read_to_string(path) else {
        return (false, false);
    };
    let installed = agent.has_hook(&content).unwrap_or(false);
    let current = installed
        && expected
            .is_some_and(|command| agent.hook_uses_command(&content, command).unwrap_or(false));
    (installed, current)
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
    let under_home = cwd.starts_with(home);
    let mut dir = cwd.to_path_buf();
    loop {
        if dir == home {
            break;
        }
        let files = project_files(agent, &dir);
        for path in &files {
            if path.is_file() && hook_state(path, agent, None).0 {
                return Ok((path.clone(), true));
            }
        }
        if let Some(found) = files.into_iter().find(|path| path.is_file()) {
            return Ok((found, false));
        }
        match dir.parent() {
            Some(parent) if under_home => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    Ok((canonical_project_path(agent, cwd), false))
}

fn find_project_skill(agent: config::Agent, home: &Path, cwd: &Path) -> Result<(PathBuf, bool)> {
    let under_home = cwd.starts_with(home);
    let mut dir = cwd.to_path_buf();
    loop {
        if dir == home {
            break;
        }
        let path = project_skill_file(agent, &dir);
        if path.is_file() && skill_state(&path).0 {
            return Ok((path, true));
        }
        if path.is_file() {
            return Ok((path, false));
        }
        match dir.parent() {
            Some(parent) if under_home => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    Ok((canonical_project_skill_path(agent, cwd), false))
}

fn find_agents_skill(home: &Path, cwd: &Path) -> Option<PathBuf> {
    let under_home = cwd.starts_with(home);
    let mut dir = cwd.to_path_buf();
    loop {
        if dir == home {
            break;
        }
        let path = dir
            .join(".agents")
            .join("skills")
            .join("lade")
            .join("SKILL.md");
        if path.is_file() {
            return Some(path);
        }
        match dir.parent() {
            Some(parent) if under_home => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    None
}

fn project_skill_file(agent: config::Agent, dir: &Path) -> PathBuf {
    match agent {
        config::Agent::Cursor => dir
            .join(".cursor")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
        config::Agent::Claude => dir
            .join(".claude")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
        config::Agent::Codex => dir
            .join(".codex")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
        config::Agent::OpenCode => dir
            .join(".opencode")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
    }
}

fn canonical_project_skill_path(agent: config::Agent, cwd: &Path) -> PathBuf {
    project_skill_file(agent, cwd)
}

fn project_files(agent: config::Agent, dir: &Path) -> Vec<PathBuf> {
    match agent {
        config::Agent::Cursor => vec![dir.join(".cursor").join("hooks.json")],
        config::Agent::Claude => vec![
            dir.join(".claude").join("settings.local.json"),
            dir.join(".claude").join("settings.json"),
        ],
        config::Agent::Codex => vec![dir.join(".codex").join("hooks.json")],
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
        config::Agent::OpenCode => cwd
            .join(".opencode")
            .join("plugins")
            .join("lade-pretool.js"),
    }
}
