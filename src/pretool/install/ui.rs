use std::io::{self, Write};
use std::path::Path;

use anyhow::{Result, bail};

use crate::message_box::Report;

use super::agent::{AGENTS, Agent};
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

fn supported_harness_list() -> String {
    let slugs: Vec<&str> = AGENTS.iter().copied().map(Agent::slug).collect();
    let Some((last, rest)) = slugs.split_last() else {
        return String::new();
    };
    if rest.is_empty() {
        return (*last).to_string();
    }
    format!("{}, or {last}", rest.join(", "))
}

pub(super) fn parse_harnesses(answer: &str) -> Result<Vec<Agent>> {
    let mut harnesses = Vec::new();
    for token in answer.split(|c: char| c == ',' || c.is_whitespace()) {
        if token.is_empty() {
            continue;
        }
        let Some(harness) = Agent::from_slug(token) else {
            bail!(
                "unknown harness '{token}'. Use {}",
                supported_harness_list()
            );
        };
        if !harnesses.contains(&harness) {
            harnesses.push(harness);
        }
    }
    if harnesses.is_empty() {
        bail!(
            "name at least one harness: {}",
            AGENTS
                .iter()
                .copied()
                .map(Agent::slug)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(harnesses)
}

/// Empty installs `detected`. `all` installs every supported harness.
pub(super) fn parse_which_harnesses(answer: &str, detected: &[Agent]) -> Result<Vec<Agent>> {
    let answer = answer.trim();
    if answer.is_empty() {
        return Ok(detected.to_vec());
    }
    if answer.eq_ignore_ascii_case("all") {
        return Ok(AGENTS.to_vec());
    }
    let lower = answer.to_ascii_lowercase();
    if lower == "n" || lower == "no" || lower == "y" || lower == "yes" {
        let local = if detected.len() == 1 {
            "harness"
        } else {
            "harnesses"
        };
        bail!(
            "name slugs, or press enter for the local {local}. Use {}",
            supported_harness_list()
        );
    }
    parse_harnesses(answer)
}

pub(super) fn which_prompt(detected: &[Agent]) -> String {
    let offered = AGENTS
        .iter()
        .copied()
        .map(Agent::slug)
        .collect::<Vec<_>>()
        .join(", ");
    let default = detected
        .iter()
        .copied()
        .map(Agent::slug)
        .collect::<Vec<_>>()
        .join(", ");
    if default.is_empty() {
        format!("Which ({offered}, or all): ")
    } else {
        format!("Which ({offered}, or all) [{default}]: ")
    }
}

pub(super) fn ask_harnesses(detected: &[Agent]) -> Result<Vec<Agent>> {
    if detected.is_empty() {
        return Ok(Vec::new());
    }
    let names = detected
        .iter()
        .copied()
        .map(Agent::name)
        .collect::<Vec<_>>()
        .join(", ");
    if !confirm_default_yes(&format!("Wrap harnesses for {names}?"))? {
        return Ok(Vec::new());
    }
    parse_which_harnesses(&read_line(&which_prompt(detected))?, detected)
}

pub(super) fn where_line(scope: Scope, home: &Path, dest: &Path) -> String {
    match scope {
        Scope::User => "this machine".to_string(),
        Scope::Project => format!("this repo  {}", tilde(dest, home)),
    }
}

pub(crate) const PREEXEC_SETUP_DIM: &str =
    "Wraps commands you type. `lade hook disable --shell` removes it.";
pub(crate) const PREEXEC_TEARDOWN_DIM: &str = "Wrap stays. `lade hook disable --shell` removes it.";
pub(crate) const PRETOOL_SETUP_DIM: &str =
    "Wraps commands harnesses run. `lade teardown` removes them from this repo.";
pub(crate) const PRETOOL_TEARDOWN_DIM: &str =
    "Removed from this repo. `lade setup` puts them back.";
pub(crate) const PREEXEC_DISABLE_DIM: &str =
    "Removed from this shell. `lade hook enable --shell` puts it back.";

pub(crate) fn print_setup(shell: &crate::preexec::SetupShell, pretool: &PretoolReport) {
    print_preexec(shell);
    print_pretool_intro(&pretool.where_line, PRETOOL_SETUP_DIM);
    print_pretool_rows(pretool, false);
}

pub(crate) fn print_preexec(shell: &crate::preexec::SetupShell) {
    let mut report = Report::new()
        .blank()
        .heading("pre-exec  this shell")
        .dim(PREEXEC_SETUP_DIM)
        .blank();
    report = append_preexec(report, shell);
    report.print();
}

pub(crate) fn print_pretool_intro(where_line: &str, dim: &str) {
    Report::new()
        .blank()
        .heading(format!("pre-tool  {where_line}"))
        .dim(dim)
        .blank()
        .print();
}

pub(crate) fn print_pretool_rows(pretool: &PretoolReport, show_verb: bool) {
    let mut report = Report::new();
    report = append_pretool_rows(report, pretool, show_verb);
    report.print();
}

pub(crate) fn print_teardown(pretool: &PretoolReport) {
    let mut report = Report::new()
        .heading("pre-exec  this shell")
        .dim(PREEXEC_TEARDOWN_DIM)
        .blank()
        .heading(format!("pre-tool  {}", pretool.where_line))
        .dim(PRETOOL_TEARDOWN_DIM)
        .blank();
    report = append_pretool_rows(report, pretool, true);
    report.print();
}

pub(crate) fn print_shell_hook(verb: &str, path: &str, reload: Option<&str>, dim: &str) {
    let mut report = Report::new()
        .heading("pre-exec  this shell")
        .dim(dim)
        .blank()
        .line(row(verb, path));
    if let Some(reload) = reload {
        report = report.dim(reload);
    }
    report.print();
}

fn append_preexec(mut report: Report, shell: &crate::preexec::SetupShell) -> Report {
    let home = super::paths::home_dir().ok();
    let listed = crate::preexec::present_shells();
    if listed.is_empty() {
        return match shell {
            crate::preexec::SetupShell::Bootstrapped { path, reload } => {
                report.line(row("installed", path)).dim(reload)
            }
            crate::preexec::SetupShell::Current { path } => report.line(row("current", path)),
            crate::preexec::SetupShell::Missing => report
                .line(row("missing", "this profile"))
                .dim("Run `lade hook enable --shell`, then reload this shell."),
            crate::preexec::SetupShell::SkippedCi => report.dim("CI. No shell wrap."),
        };
    }
    for sh in &listed {
        let (path, _) = crate::preexec::preexec_installed(sh);
        let shown = match &home {
            Some(home) => tilde(&path, home),
            None => path.display().to_string(),
        };
        report = report.line(row(sh.display_name(), &shown));
    }
    match shell {
        crate::preexec::SetupShell::Bootstrapped { reload, .. } => report.dim(reload),
        crate::preexec::SetupShell::Missing => {
            report.dim("Run `lade hook enable --shell`, then reload this shell.")
        }
        crate::preexec::SetupShell::SkippedCi => report.dim("CI. No shell wrap."),
        crate::preexec::SetupShell::Current { .. } => report,
    }
}

fn append_pretool_rows(mut report: Report, pretool: &PretoolReport, show_verb: bool) -> Report {
    if pretool.rows.is_empty() {
        return report.dim("nothing here.");
    }
    for item in &pretool.rows {
        let note = if !item.note.is_empty() {
            item.note
        } else if item.verb == ItemVerb::Flagged {
            "home leftover"
        } else {
            ""
        };
        let value = if show_verb {
            format!("{} {}", item.verb.label(), item.path)
        } else {
            item.path.clone()
        };
        report = report.line(row(item.agent, &value));
        if !note.is_empty() {
            report = report.dim(format!("               {note}"));
        }
    }
    report
}

fn row(name: &str, value: &str) -> String {
    format!("  {name:<12}{value}")
}

pub(super) fn report(title: &str, dim: &str, results: Vec<String>) {
    if results.is_empty() {
        return;
    }
    let mut out = Report::new().heading(title).dim(dim);
    for result in results {
        out = out.line(format!("  {result}"));
    }
    out.print();
}
