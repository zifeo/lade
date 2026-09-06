use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, SecondsFormat, Utc};
use flate2::Compression;
use flate2::write::GzEncoder;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use tar::Builder;

use crate::event::{Event, events_ddl, query as query_live};
use crate::global_config::GlobalConfig;
use crate::message_box;

use super::basename_or_self;

pub fn share(
    since: Option<&DateTime<Utc>>,
    until: Option<&DateTime<Utc>>,
    limit: Option<usize>,
    audience: Option<&str>,
    kind: Option<&str>,
    repo: Option<&str>,
    output: Option<&Path>,
) -> Result<()> {
    let rows = query_live(since, until, limit, audience, kind, repo)?;
    let home = home_dir();
    let redacted: Vec<Event> = rows
        .iter()
        .filter_map(|row| redact_for_share(row, &home))
        .collect();
    let now = Utc::now();
    let to_ts = until.copied().unwrap_or(now);
    let from_label = match since {
        Some(ts) => compact_utc(*ts),
        None => "all".to_string(),
    };
    let to_label = compact_utc(to_ts);
    let manifest = json!({
        "v": 1,
        "exported_at": now.to_rfc3339_opts(SecondsFormat::Millis, true),
        "since": since
            .map(|ts| ts.to_rfc3339_opts(SecondsFormat::Millis, true))
            .unwrap_or_else(|| "unbounded".to_string()),
        "until": until
            .map(|ts| ts.to_rfc3339_opts(SecondsFormat::Millis, true))
            .unwrap_or_else(|| now.to_rfc3339_opts(SecondsFormat::Millis, true)),
        "count": redacted.len(),
        "from": from_label,
        "to": to_label,
    });
    let out_path = match output {
        Some(path) => path.to_path_buf(),
        None => {
            let user = share_user();
            std::env::current_dir()?
                .join(format!("lade-{}-{}-{}.tar.gz", user, from_label, to_label))
        }
    };
    if out_path.exists() {
        anyhow::bail!("refusing to overwrite {}", out_path.display());
    }
    let work = tempfile::Builder::new()
        .prefix("lade-log-share-")
        .tempdir()?;
    let manifest_path = work.path().join("manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    let snapshot_path = work.path().join("events.db");
    write_snapshot_db(&redacted, &snapshot_path)?;
    write_tar_gz(&out_path, &manifest_path, &snapshot_path)?;
    message_box::MessageBox::new()
        .info()
        .line(out_path.display().to_string())
        .print_plain_stderr();
    Ok(())
}

fn write_snapshot_db(events: &[Event], path: &Path) -> Result<()> {
    let _ = fs::remove_file(path);
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=OFF")?;
    conn.execute_batch(&events_ddl())?;
    for event in events {
        let matches = serde_json::to_string(&event.matches).unwrap_or_else(|_| "[]".into());
        let agent = event
            .agent
            .as_ref()
            .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "null".into()));
        conn.execute(
            "INSERT INTO events (
                id, ts, kind, via, audience, actor, repo, git_commit,
                command, command_truncated, hydrate_ms, matches, agent
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, jsonb(?12), jsonb(?13))",
            params![
                event.id,
                event.ts,
                event.kind,
                event.via,
                event.audience,
                event.actor,
                event.repo,
                event.git_commit,
                event.command,
                event.command_truncated as i64,
                event.hydrate_ms,
                matches,
                agent,
            ],
        )?;
    }
    Ok(())
}

fn write_tar_gz(out: &Path, manifest: &Path, events_db: &Path) -> Result<()> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(out)?;
    let enc = GzEncoder::new(file, Compression::new(6));
    let mut tar = Builder::new(enc);
    tar.append_path_with_name(manifest, "manifest.json")?;
    tar.append_path_with_name(events_db, "events.db")?;
    let inner = tar.into_inner()?;
    inner.finish()?;
    Ok(())
}

pub(super) fn redact_for_share(event: &Event, home: &Path) -> Option<Event> {
    if event.command.is_empty() {
        return None;
    }
    let git_root = event.repo.as_deref();
    let command = strip_prefixes(&event.command, home, git_root);
    let repo = event.repo.as_ref().map(|r| basename_or_self(r));
    let matches = redact_matches(&event.matches);
    Some(Event {
        id: event.id.clone(),
        ts: event.ts.clone(),
        kind: event.kind.clone(),
        via: event.via.clone(),
        audience: event.audience.clone(),
        actor: None,
        repo,
        git_commit: event.git_commit.clone(),
        command,
        command_truncated: event.command_truncated,
        hydrate_ms: event.hydrate_ms,
        matches,
        agent: event.agent.clone(),
    })
}

fn strip_prefixes(command: &str, home: &Path, git_root: Option<&str>) -> String {
    let mut cmd = command.to_string();
    let home_str = home.to_string_lossy();
    if let Some(rest) = cmd.strip_prefix(home_str.as_ref()) {
        cmd = rest.trim_start_matches('/').to_string();
    }
    if let Some(root) = git_root
        && let Some(rest) = cmd.strip_prefix(root)
    {
        cmd = rest.trim_start_matches('/').to_string();
    }
    cmd
}

fn redact_matches(matches: &Value) -> Value {
    let Some(items) = matches.as_array() else {
        return matches.clone();
    };
    let out = items
        .iter()
        .map(|item| {
            let mut obj = item.clone();
            if let Some(map) = obj.as_object_mut()
                && let Some(file) = map.get("file").and_then(|f| f.as_str())
            {
                map.insert("file".into(), json!(basename_or_self(file)));
            }
            obj
        })
        .collect();
    Value::Array(out)
}

fn share_user() -> String {
    let raw = GlobalConfig::user_from_disk()
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_else(|| "unknown".to_string());
    sanitize_filename_component(&raw)
}

pub(super) fn sanitize_filename_component(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn compact_utc(ts: DateTime<Utc>) -> String {
    ts.format("%Y%m%dT%H%M%SZ").to_string()
}

fn home_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home);
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(home);
    }
    PathBuf::from("/")
}
