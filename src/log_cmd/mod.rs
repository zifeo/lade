mod log;
mod usage;

pub use log::run_log;
pub use usage::run_usage;

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::event;
use crate::log_pack;
use crate::message_box;
use crate::window;

const DEFAULT_SINCE: &str = "90d";

fn fetch_events(
    sources: &[String],
    since: Option<&chrono::DateTime<chrono::Utc>>,
    until: Option<&chrono::DateTime<chrono::Utc>>,
    limit: Option<usize>,
    audience: Option<&str>,
    kind: Option<&str>,
    repo: Option<&str>,
) -> Result<Vec<event::Event>> {
    let result = if sources.is_empty() {
        event::query(since, until, limit, audience, kind, repo).map_err(Into::into)
    } else {
        log_pack::query_sources(sources, since, until, limit, audience, kind, repo)
    };
    match result {
        Ok(rows) => Ok(rows),
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .paragraph(e.to_string())
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    }
}

fn repo_filter(all: bool, path: Option<&Path>, cwd: &Path) -> Option<String> {
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
    if path.is_some() && repo.is_none() {
        message_box::MessageBox::new()
            .error()
            .line(format!("no git root at {}", start.display()))
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    }
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
