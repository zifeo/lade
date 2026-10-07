use anyhow::Result;
use std::path::PathBuf;

use super::{fetch_events, filter_opt, query_window, repo_filter};
use crate::args::{LogAction, LogCommand};
use crate::catalog;
use crate::event::pack;
use crate::event::{self, Event};
use crate::message_box;
use crate::window;

pub fn run_log(opts: LogCommand, agent: bool) -> Result<()> {
    if !opts.source.is_empty() {
        if matches!(opts.action, Some(LogAction::Share { .. })) {
            message_box::MessageBox::new()
                .error()
                .line("--source cannot be used with share")
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
        if matches!(opts.action, Some(LogAction::Prune { .. })) {
            message_box::MessageBox::new()
                .error()
                .line("--source cannot be used with prune")
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    }
    match opts.action {
        Some(LogAction::Share { ref output }) => {
            return run_share(&opts, agent, output.clone());
        }
        Some(LogAction::Prune { ref keep, hub }) => {
            return run_prune(&opts, keep.as_deref(), hub);
        }
        Some(LogAction::Verify) => return run_verify(&opts),
        None => {}
    }
    let group = match opts.group.as_deref() {
        None => None,
        Some("command") => Some("command"),
        Some(other) => {
            message_box::MessageBox::new()
                .error()
                .line(format!("unknown --group {other}. use command"))
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    let (since, until, limit) =
        query_window(opts.since.as_deref(), opts.until.as_deref(), opts.limit)?;
    let audience = filter_opt(&opts.audience);
    let kind = filter_opt(&opts.kind);
    let cwd = std::env::current_dir()?;
    let repo = repo_filter(opts.all || opts.global, opts.path.as_deref(), &cwd);
    let event_limit = if group.is_some() { None } else { limit };
    let rows = fetch_events(
        &opts.source,
        since.as_ref(),
        until.as_ref(),
        event_limit,
        audience,
        kind,
        repo.as_deref(),
    )?;
    if group == Some("command") {
        let mut grouped = catalog::group_commands(&rows);
        if let Some(n) = limit {
            grouped.truncate(n);
        }
        if opts.json {
            println!("{}", serde_json::to_string_pretty(&grouped)?);
        } else {
            for row in grouped {
                println!("{}  {}", row.count, row.command);
            }
        }
        return Ok(());
    }
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for row in rows {
            println!(
                "{} {} {} {}",
                row.ts,
                row.kind,
                row.via.as_deref().unwrap_or("-"),
                log_command(&row)
            );
        }
    }
    Ok(())
}

fn log_command(row: &Event) -> String {
    crate::event::display_line(&row.command, row.argv.as_ref()).unwrap_or_else(|| {
        row.agent
            .as_ref()
            .and_then(|agent| agent.get("launch"))
            .and_then(|value| value.as_str())
            .unwrap_or("-")
            .to_string()
    })
}

fn run_verify(opts: &LogCommand) -> Result<()> {
    let reports = super::chain_reports(&opts.source)?;
    let mut ok = true;
    let mut lines = Vec::new();
    for (path, status) in reports {
        if !status.ok {
            ok = false;
        }
        lines.extend(status.report_lines(path.display()));
    }
    if ok {
        let mut report = message_box::Report::new();
        for line in lines {
            report = report.line(line);
        }
        report.print();
        return Ok(());
    }
    let mut box_ = message_box::MessageBox::new().error();
    for line in lines {
        box_ = box_.line(line);
    }
    box_.print_stderr();
    std::process::exit(crate::exit_codes::FAILURE);
}

fn run_share(opts: &LogCommand, agent: bool, output: Option<PathBuf>) -> Result<()> {
    if agent {
        message_box::MessageBox::new()
            .error()
            .line("lade log share is not available in agent sessions")
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    }
    let (since, until, limit) =
        query_window(opts.since.as_deref(), opts.until.as_deref(), opts.limit)?;
    let audience = filter_opt(&opts.audience);
    let kind = filter_opt(&opts.kind);
    let cwd = std::env::current_dir()?;
    let repo = repo_filter(opts.all || opts.global, opts.path.as_deref(), &cwd);
    match pack::share(
        since.as_ref(),
        until.as_ref(),
        limit,
        audience,
        kind,
        repo.as_deref(),
        output.as_deref(),
    ) {
        Ok(()) => Ok(()),
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .paragraph(e.to_string())
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    }
}

fn run_prune(opts: &LogCommand, keep: Option<&str>, hub: bool) -> Result<()> {
    if keep.is_none() && !hub {
        message_box::MessageBox::new()
            .error()
            .line("need --keep or --hub")
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    }
    let mut report = message_box::Report::new();
    if let Some(keep) = keep {
        let ts = match window::cutoff(keep) {
            Ok(ts) => ts,
            Err(e) => {
                message_box::MessageBox::new()
                    .error()
                    .paragraph(e)
                    .print_stderr();
                std::process::exit(crate::exit_codes::FAILURE);
            }
        };
        let cwd = std::env::current_dir()?;
        let repo = repo_filter(opts.all || opts.global, opts.path.as_deref(), &cwd);
        let n = match event::prune_before(&ts, repo.as_deref()) {
            Ok(n) => n,
            Err(e) => {
                message_box::MessageBox::new()
                    .error()
                    .paragraph(e.to_string())
                    .print_stderr();
                std::process::exit(crate::exit_codes::FAILURE);
            }
        };
        report = report.line(format!("deleted {n} rows older than {keep}"));
    }
    if hub {
        report = report.line(match crate::hub::stop() {
            crate::hub::HubStop::Stopped => "hub: stopped".to_string(),
            crate::hub::HubStop::Down => "hub: down".to_string(),
            crate::hub::HubStop::Off => "hub: off".to_string(),
            crate::hub::HubStop::Stale => "hub: stale".to_string(),
        });
    }
    report.print();
    Ok(())
}
