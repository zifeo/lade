use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, SecondsFormat, Utc};
use flate2::Compression;
use flate2::write::GzEncoder;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use tar::Builder;

use crate::event::{Event, events_ddl, insert_sealed, query as query_live};
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
    let work = crate::cache::scratch_tempdir()?;
    let snapshot_path = work.path().join("events.db");
    let head = write_snapshot_db(&redacted, &snapshot_path)?;
    let manifest = json!({
        "v": 3,
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
        "chain": {
            "alg": "hmac-sha256-v1",
            "head": head,
            "rows": redacted.len(),
        },
    });
    let manifest_path = work.path().join("manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    write_tar_gz(&out_path, &manifest_path, &snapshot_path)?;
    message_box::Report::new()
        .line(out_path.display().to_string())
        .print();
    Ok(())
}

fn write_snapshot_db(events: &[Event], path: &Path) -> Result<Option<String>> {
    let _ = fs::remove_file(path);
    let mut conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=OFF")?;
    conn.execute_batch(&events_ddl())?;
    let tx = conn.transaction()?;
    for event in events.iter().rev() {
        insert_sealed(&tx, event)?;
    }
    let head = tx
        .query_row("SELECT head_hash FROM chain_head WHERE id = 1", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()?;
    tx.commit()?;
    Ok(head)
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
    if event.command.is_empty() && event.via.as_deref() != Some("mcp") {
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
        argv: event.argv.clone(),
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
