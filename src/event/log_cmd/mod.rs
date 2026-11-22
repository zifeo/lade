mod log;
mod usage;

pub use log::run_log;
pub use usage::run_usage;

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::event;
use crate::event::pack;
use crate::message_box;
use crate::window;

const DEFAULT_SINCE: &str = "90d";

fn chain_reports(sources: &[String]) -> Result<Vec<(PathBuf, event::ChainStatus)>> {
    if sources.is_empty() {
        Ok(vec![(event::db_path(), event::verify_live()?)])
    } else {
        pack::verify_sources(sources)
    }
}

fn ensure_chain(sources: &[String]) -> Result<()> {
    // § bqy4mdd5m.a@8cer2
    let reports = match chain_reports(sources) {
        Ok(reports) => reports,
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .paragraph(e.to_string())
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    let lines: Vec<String> = reports
        .iter()
        .filter(|(_, status)| !status.ok)
        .flat_map(|(path, status)| status.report_lines(path.display()))
        .collect();
    if lines.is_empty() {
        return Ok(());
    }
    let mut box_ = message_box::MessageBox::new().warning();
    for line in lines {
        box_ = box_.line(line);
    }
    box_.print_stderr();
    Ok(())
}

fn fetch_events(
    sources: &[String],
    since: Option<&chrono::DateTime<chrono::Utc>>,
    until: Option<&chrono::DateTime<chrono::Utc>>,
    limit: Option<usize>,
    audience: Option<&str>,
    kind: Option<&str>,
    repo: Option<&str>,
) -> Result<Vec<event::Event>> {
    ensure_chain(sources)?;
    let result = if sources.is_empty() {
        event::query(since, until, limit, audience, kind, repo).map_err(Into::into)
    } else {
        pack::query_sources(sources, since, until, limit, audience, kind, repo)
    };
    match result {
        Ok(rows) => Ok(one_row_per_call(rows)),
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .paragraph(e.to_string())
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    }
}

fn call_id(row: &event::Event) -> Option<&str> {
    row.agent
        .as_ref()
        .and_then(|agent| agent.get("tool_use_id"))
        .and_then(|id| id.as_str())
        .filter(|id| !id.is_empty())
}

/// The diary is append-only, so a second hook that could not fill the row of
/// its MCP call appended a full copy: the newest row stands for that call.
fn one_row_per_call(rows: Vec<event::Event>) -> Vec<event::Event> {
    // § bvttr6qk1.e@3vh3n
    let mut newest: std::collections::HashMap<&str, (&str, &str)> =
        std::collections::HashMap::new();
    for row in &rows {
        if let Some(call) = call_id(row) {
            let seen = newest
                .entry(call)
                .or_insert((row.ts.as_str(), row.id.as_str()));
            if (row.ts.as_str(), row.id.as_str()) > *seen {
                *seen = (row.ts.as_str(), row.id.as_str());
            }
        }
    }
    let keep: std::collections::HashSet<String> =
        newest.values().map(|(_, id)| id.to_string()).collect();
    rows.into_iter()
        .filter(|row| call_id(row).is_none() || keep.contains(&row.id))
        .collect()
}

fn repo_filter(all: bool, path: Option<&Path>, cwd: &Path) -> Option<String> {
    // § ck86fwnt7.b@tthy5
    if all {
        return None;
    }
    let start = match path {
        Some(raw) => {
            let start = expand_user_path(raw);
            if !start.is_dir() {
                message_box::MessageBox::new()
                    .error()
                    .line(format!("--path is not a directory: {}", start.display()))
                    .print_stderr();
                std::process::exit(crate::exit_codes::FAILURE);
            }
            start.canonicalize().unwrap_or(start)
        }
        None => cwd.to_path_buf(),
    };
    let repo = crate::catalog::git_root(&start).map(|p| p.to_string_lossy().into_owned());
    // § ck86fwnt7.c@anfyn
    if repo.is_none() {
        message_box::MessageBox::new()
            .error()
            .line(format!(
                "no git root at {}. Use --all to read every repo.",
                start.display()
            ))
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    }
    // § end ck86fwnt7.c
    repo
}

fn expand_user_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~"
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home);
    }
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    path.to_path_buf()
}

type QueryWindow = (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<usize>,
);

fn query_window(
    since: Option<&str>,
    until: Option<&str>,
    limit: Option<usize>,
) -> Result<QueryWindow> {
    let since = match since {
        Some(raw) => Some(parse_cutoff(raw)?),
        None if limit.is_none() => Some(parse_cutoff(DEFAULT_SINCE)?),
        None => None,
    };
    let until = match until {
        Some(raw) => Some(parse_cutoff(raw)?),
        None => None,
    };
    Ok((since, until, limit))
}

fn parse_cutoff(raw: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    match window::cutoff(raw) {
        Ok(ts) => Ok(ts),
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .paragraph(e)
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    }
}

fn filter_opt(raw: &str) -> Option<&str> {
    match raw {
        "all" | "" => None,
        other => Some(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn verb(command: &str, agent: serde_json::Value) -> event::Emit {
        event::Emit {
            kind: event::Kind::Seen,
            via: crate::audience::Via::Mcp,
            audience: crate::config::Audience::Agent,
            actor: None,
            cwd: PathBuf::from("."),
            command: command.into(),
            argv: None,
            hydrated: None,
            matches: json!([]),
            hydrate_ms: None,
            agent: Some(agent),
        }
    }

    #[test]
    fn a_call_appended_twice_counts_once_newest_wins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        temp_env::with_var("LADE_EVENTS_PATH", Some(path.to_str().unwrap()), || {
            event::emit_verb(
                true,
                Some("preToolUse"),
                Some("toolu_9"),
                verb(
                    "engram.mem_stats",
                    json!({"tool_use_id": "toolu_9", "hook": "preToolUse"}),
                ),
            );
            event::emit(verb("other.call", json!({"tool_use_id": "toolu_10"})));
            event::emit_verb(
                true,
                Some("beforeMCPExecution"),
                Some("toolu_9"),
                verb(
                    "engram.mem_stats",
                    json!({"tool_use_id": "toolu_9", "launch": "engram"}),
                ),
            );
            let rows = event::query(None, None, None, None, None, None).unwrap();
            assert_eq!(rows.len(), 3, "the diary stays append-only");
            let rows = one_row_per_call(rows);
            assert_eq!(rows.len(), 2);
            let call = rows
                .iter()
                .find(|row| call_id(row) == Some("toolu_9"))
                .unwrap();
            assert_eq!(call.agent.as_ref().unwrap()["launch"], "engram");
        });
    }
}
