use std::io::{self, Write};
use std::path::Path;

use anyhow::{Result, bail};

use crate::message_box::MessageBox;

use super::agent::Agent;
use super::paths::{ItemVerb, tilde};
use super::write::Scope;

pub(crate) struct PretoolReport {
    pub where_line: String,
    pub rows: Vec<PretoolRow>,
}

pub(crate) struct PretoolRow {
    pub agent: &'static str,
    pub verb: ItemVerb,
    pub path: String,
}

pub(super) fn read_line(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(answer.trim().to_string())
}

pub(super) fn confirm_default_yes(prompt: &str) -> Result<bool> {
    let answer = read_line(&format!("{prompt} [Y/n]: "))?;
    parse_yes_no(&answer, true)
}

pub(super) fn parse_yes_no(answer: &str, default_yes: bool) -> Result<bool> {
    let answer = answer.trim().to_ascii_lowercase();
    if answer.is_empty() {
        return Ok(default_yes);
    }
    if answer == "y" || answer == "yes" {
        return Ok(true);
    }
    if answer == "n" || answer == "no" {
        return Ok(false);
    }
    bail!("type Y or n");
}

pub(super) fn default_scope(in_git: bool) -> Scope {
    if in_git { Scope::Project } else { Scope::User }
}

pub(super) fn parse_scope(answer: &str, default: Scope) -> Result<Scope> {
    match answer.trim().to_ascii_lowercase().as_str() {
        "" | "y" | "yes" => Ok(default),
        "m" | "machine" | "u" | "user" => Ok(Scope::User),
        "p" | "project" | "l" | "local" | "repo" => Ok(Scope::Project),
        _ => bail!("type Y for this repo or m for this machine"),
    }
}

pub(super) fn parse_agents(answer: &str) -> Result<Vec<Agent>> {
    let mut agents = Vec::new();
    for token in answer.split(|c: char| c == ',' || c.is_whitespace()) {
        if token.is_empty() {
            continue;
        }
        let Some(agent) = Agent::from_slug(token) else {
            bail!("unknown agent '{token}'. Use cursor, claude, codex, or opencode");
        };
        if !agents.contains(&agent) {
            agents.push(agent);
        }
    }
    if agents.is_empty() {
        bail!("name at least one agent: cursor, claude, codex, opencode");
    }
    Ok(agents)
}

pub(super) fn ask_repo_or_machine() -> Result<Scope> {
    parse_scope(
        &read_line("Install Lade in this repo? [Y] this repo / [m] this machine: ")?,
        Scope::Project,
    )
}

pub(super) fn ask_agents(detected: &[Agent]) -> Result<Vec<Agent>> {
    if detected.is_empty() {
        return parse_agents(&read_line(
            "No agents detected. Agents (cursor, claude, codex, opencode): ",
        )?);
    }
    let names = detected
        .iter()
        .copied()
        .map(Agent::name)
        .collect::<Vec<_>>()
        .join(", ");
    if confirm_default_yes(&format!("Install for {names}?"))? {
        return Ok(detected.to_vec());
    }
    parse_agents(&read_line("Agents (cursor, claude, codex, opencode): ")?)
}

pub(super) fn warn_double_hooks(names: &[String]) {
    if names.is_empty() {
        return;
    }
    MessageBox::new()
        .warning()
        .line("This machine already has Lade hooks for:")
        .line(format!("- {}", names.join(", ")))
        .line("This repo's hooks and the machine hooks will both run.")
        .line("The second hook skips rewrite. You still pay two processes.")
        .line("Remove the machine hooks with `lade hook uninstall --scope user --harness <slug>`.")
        .print_stderr();
}

pub(super) fn where_line(scope: Scope, home: &Path, dest: &Path) -> String {
    match scope {
        Scope::User => "this machine".to_string(),
        Scope::Project => format!("this repo  {}", tilde(dest, home)),
    }
}

pub(crate) fn print_setup(found: &str, verb: &str, path: &str, pretool: &PretoolReport) {
    let mut mb = MessageBox::new()
        .info()
        .line("pre-exec  this shell")
        .line("Wraps commands you type.")
        .line("")
        .line(found)
        .line(format!("  {verb:<9}  {path}"))
        .line("")
        .line(format!("pre-tool  {}", pretool.where_line))
        .line("Wraps commands agents run. Hook and skill together.")
        .line("");
    if pretool.rows.is_empty() {
        mb = mb.line("nothing here.");
    } else {
        let mut last = None;
        for row in &pretool.rows {
            if last != Some(row.agent) {
                mb = mb.line(row.agent);
                last = Some(row.agent);
            }
            let note = if row.verb == ItemVerb::Unmanaged {
                "  not Lade-managed"
            } else {
                ""
            };
            mb = mb.line(format!("  {:<9}  {}{note}", row.verb.label(), row.path));
        }
    }
    mb.print_stderr();
}

pub(super) fn report(title: &str, results: Vec<String>) {
    if results.is_empty() {
        return;
    }
    let mut mb = MessageBox::new().info().line(title);
    for result in results {
        mb = mb.line(format!("- {result}"));
    }
    mb.print_plain_stderr();
}
