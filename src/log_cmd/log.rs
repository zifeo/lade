use anyhow::Result;
use std::path::PathBuf;

use super::{fetch_events, filter_opt, query_window, repo_filter};
use crate::args::{LogAction, LogCommand};
use crate::catalog;
use crate::event::{self, Event};
use crate::log_pack;
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
        Some(LogAction::Prune { keep }) => return run_prune(keep.as_deref()),
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
    let repo = repo_filter(opts.all, opts.path.as_deref(), &cwd);
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
    let repo = repo_filter(opts.all, opts.path.as_deref(), &cwd);
    match log_pack::share(
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

fn run_prune(keep: Option<&str>) -> Result<()> {
    let Some(keep) = keep else {
        message_box::MessageBox::new()
            .error()
            .line("need --keep")
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    };
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
    let n = match event::prune_before(&ts) {
        Ok(n) => n,
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .paragraph(e.to_string())
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    message_box::MessageBox::new()
        .info()
        .line(format!("deleted {n} rows older than {keep}"))
        .print_plain_stderr();
    Ok(())
}
