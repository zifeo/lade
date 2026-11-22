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
    // § abp3kwm4w.a@qm08h
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
        None => std::env::current_dir()?.join(format!("lade-{from_label}-{to_label}.tar.gz")),
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
    // § abp3kwm4w.d@j6dzt
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
    // § one abp3kwm4w.c@axjhx
    if event.command.is_empty() && event.via.as_deref() != Some("mcp") {
        return None;
    }
    // § abp3kwm4w.b@qanjd
    let home = home.to_string_lossy();
    let places = Places {
        repo: event.repo.as_deref(),
        home: &home,
    };
    Some(Event {
        id: event.id.clone(),
        ts: event.ts.clone(),
        kind: event.kind.clone(),
        via: event.via.clone(),
        audience: event.audience.clone(),
        actor: None,
        repo: event.repo.as_deref().map(basename_or_self),
        git_commit: event.git_commit.clone(),
        command: share_text(&places, &event.command),
        command_truncated: event.command_truncated,
        argv: event.argv.as_ref().map(|argv| share_value(&places, argv)),
        hydrate_ms: event.hydrate_ms,
        matches: share_value(&places, &event.matches),
        agent: event
            .agent
            .as_ref()
            .map(|agent| share_value(&places, agent)),
    })
}

/// The directories a packed row names its paths from: the repo it ran in, then the home.
pub(super) struct Places<'a> {
    pub(super) repo: Option<&'a str>,
    pub(super) home: &'a str,
}

pub(super) fn share_value(places: &Places, value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(share_text(places, text)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| share_value(places, item)).collect())
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), share_value(places, item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `text` with each absolute path renamed by `share_path`. A path opens the text, follows a
/// separator or a short flag (`-I/usr/include`); the repo or the home counts anywhere.
pub(super) fn share_text(places: &Places, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(c) = text[at..].chars().next() {
        if c != '/' {
            out.push(c);
            at += c.len_utf8();
            continue;
        }
        let rest = &text[at..];
        let end = rest.find(closes_path).unwrap_or(rest.len());
        let token = &rest[..end];
        let before = &text[..at];
        let word = before.rsplit(opens_path).next().unwrap_or_default();
        let flag = word
            .strip_prefix('-')
            .is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic()));
        if !(word.is_empty() || flag || under_places(places, token)) {
            out.push(c);
            at += 1;
            continue;
        }
        match token.strip_prefix("//") {
            // `file:///p` names the path `/p`; `https://host/p` names another machine.
            Some(path) if text[..at].ends_with("file:") && path.starts_with('/') => {
                out.push_str("//");
                out.push_str(&share_path(places, path));
            }
            Some(_) => out.push_str(token),
            None => out.push_str(&share_path(places, token)),
        }
        at += end;
    }
    out
}

/// `path` relative to the repo, else as `~/...` under the home, else its last component.
pub(super) fn share_path(places: &Places, path: &str) -> String {
    if let Some(rest) = places.repo.and_then(|repo| strip_dir(path, repo)) {
        return if rest.is_empty() {
            ".".to_string()
        } else {
            rest
        };
    }
    if let Some(rest) = strip_dir(path, places.home) {
        return if rest.is_empty() {
            "~".to_string()
        } else {
            format!("~/{rest}")
        };
    }
    basename_or_self(path)
}

fn under_places(places: &Places, path: &str) -> bool {
    places
        .repo
        .is_some_and(|repo| strip_dir(path, repo).is_some())
        || strip_dir(path, places.home).is_some()
}

fn opens_path(c: char) -> bool {
    c.is_whitespace() || "=:'\"`,;([{<>|&^".contains(c)
}

fn closes_path(c: char) -> bool {
    c.is_whitespace() || "'\"`:,;)]}|&<>?#".contains(c)
}

/// `path` without the directory `dir`: `/Users/sam` is no prefix of `/Users/samantha`.
fn strip_dir(path: &str, dir: &str) -> Option<String> {
    let rest = path.strip_prefix(dir.trim_end_matches('/'))?;
    (rest.is_empty() || rest.starts_with('/')).then(|| rest.trim_start_matches('/').to_string())
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
