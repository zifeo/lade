use std::io::{self, Write};
use std::path::Path;

use anyhow::{Result, bail};

use crate::message_box::MessageBox;

use super::agent::Agent;
use super::paths::{ItemVerb, tilde};
use super::write::Scope;

#[derive(Debug)]
pub(crate) struct PretoolReport {
    pub where_line: String,
    pub rows: Vec<PretoolRow>,
}

#[derive(Debug)]
pub(crate) struct PretoolRow {
    pub agent: &'static str,
    pub verb: ItemVerb,
    pub path: String,
    pub note: &'static str,
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

pub(super) fn ask_agents(detected: &[Agent]) -> Result<Vec<Agent>> {
    if detected.is_empty() {
        return Ok(Vec::new());
    }
    let names = detected
        .iter()
        .copied()
        .map(Agent::name)
        .collect::<Vec<_>>()
        .join(", ");
    if confirm_default_yes(&format!("Wrap agents for {names}?"))? {
        return Ok(detected.to_vec());
    }
    parse_agents(&read_line("Agents (cursor, claude, codex, opencode): ")?)
}

pub(super) fn where_line(scope: Scope, home: &Path, dest: &Path) -> String {
    match scope {
        Scope::User => "this machine".to_string(),
        Scope::Project => format!("this repo  {}", tilde(dest, home)),
    }
}

pub(crate) fn print_setup(shell: &crate::shell::SetupShell, pretool: &PretoolReport) {
    let mut mb = MessageBox::new().info().line("pre-exec  this shell");
    mb = mb.line("Wraps commands you type.").line("");
    match shell {
        crate::shell::SetupShell::Bootstrapped {
            found,
            path,
            reload,
        } => {
            mb = mb
                .line(found)
                .line(format!("  {:<9}  {path}", "installed"))
                .line(reload);
        }
        crate::shell::SetupShell::Current { found, path } => {
            mb = mb.line(found).line(format!("  {:<9}  {path}", "current"));
        }
        crate::shell::SetupShell::Missing { found } => {
            mb = mb
                .line(found)
                .line("  missing   this profile")
                .line("Run `lade hook enable --shell`, then reload this shell.");
        }
        crate::shell::SetupShell::SkippedCi { found } => {
            mb = mb.line(found).line("  skipped   CI. No shell wrap.");
        }
    }
    mb = mb
        .line("")
        .line(format!("pre-tool  {}", pretool.where_line))
        .line("Wraps commands agents run.")
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
            let note = if !row.note.is_empty() {
                row.note
            } else if row.verb == ItemVerb::Flagged {
                "home leftover"
            } else {
                ""
            };
            mb = mb.line(format!("  {:<9}  {}", row.verb.label(), row.path));
            if !note.is_empty() {
                mb = mb.line(format!("            {note}"));
            }
        }
    }
    mb.print_stderr();
}

pub(crate) fn print_teardown(pretool: &PretoolReport) {
    let mut mb = MessageBox::new()
        .info()
        .line("pre-exec  this shell")
        .line("Wrap stays. `lade hook disable --shell` removes it.")
        .line("")
        .line(format!("pre-tool  {}", pretool.where_line))
        .line("Wraps commands agents run.")
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
            mb = mb.line(format!("  {:<9}  {}", row.verb.label(), row.path));
            if !row.note.is_empty() {
                mb = mb.line(format!("            {}", row.note));
            }
        }
    }
    mb.print_stderr();
}

pub(crate) fn print_shell_hook(found: &str, verb: &str, path: &str, reload: Option<&str>) {
    let mut mb = MessageBox::new()
        .info()
        .line("pre-exec  this shell")
        .line("Wraps commands you type.")
        .line("")
        .line(found)
        .line(format!("  {verb:<9}  {path}"));
    if let Some(reload) = reload {
        mb = mb.line(reload);
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
