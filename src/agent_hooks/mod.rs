//! Optional installation of the `lade hook` interceptor into the agents that
//! support `preToolUse` shell hooks (Cursor, Claude Code).
//!
//! `lade install` is a global, once-only operation, so these hooks are written
//! to the agents' global config (`~/.cursor/hooks.json`,
//! `~/.claude/settings.json`). We only act when the agent's home dir already
//! exists (i.e. the agent is actually used) and never overwrite unrelated
//! settings: the config is parsed, our entry merged idempotently (see
//! `config.rs`), and removed symmetrically on `lade uninstall`.

mod config;
#[cfg(test)]
mod tests;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::message_box::MessageBox;
use config::AGENTS;

fn home_dir() -> Result<PathBuf> {
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("cannot determine home directory")
}

fn hook_command() -> Result<String> {
    let exe = std::env::current_exe()?;
    Ok(format!("{} hook", exe.display()))
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
    MessageBox::new()
        .info()
        .line("Agent hooks")
        .paragraphs(results)
        .print_stderr();
}

/// Offer to install the `lade hook` interceptor for every agent detected on the
/// machine. `may_prompt` must be true only when both stdin and stderr are TTYs.
pub fn install(may_prompt: bool) -> Result<()> {
    let command = hook_command()?;
    let home = home_dir()?;
    let mut results = Vec::new();

    for agent in AGENTS {
        if !agent.home_dir(&home).is_dir() {
            continue;
        }
        let path = agent.config_path(&home);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        if agent.has_hook(&existing)? {
            results.push(format!("{}: hook already present", agent.name()));
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
        let existing = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        if !agent.has_hook(&existing)? {
            continue;
        }
        fs::write(&path, agent.remove(&existing)?)?;
        results.push(format!(
            "{}: hook removed from {}",
            agent.name(),
            tilde(&path, &home)
        ));
    }

    report(results);
    Ok(())
}
