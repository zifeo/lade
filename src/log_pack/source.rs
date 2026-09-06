use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use flate2::read::GzDecoder;
use rusqlite::{Connection, params};
use tar::Archive;
use tempfile::TempDir;

use crate::event::{Event, db_path, event_from_row};

use super::basename_or_self;

const PACK_PREFIX: &str = "lade-log-src-";

pub fn query_sources(
    sources: &[String],
    since: Option<&DateTime<Utc>>,
    until: Option<&DateTime<Utc>>,
    limit: Option<usize>,
    audience: Option<&str>,
    kind: Option<&str>,
    repo: Option<&str>,
) -> Result<Vec<Event>> {
    let cwd = std::env::current_dir()?;
    let expanded = expand_sources(sources, &cwd)?;
    query_attached(&expanded.paths, since, until, limit, audience, kind, repo)
}

struct ExpandedSources {
    paths: Vec<PathBuf>,
    _temp_dirs: Vec<TempDir>,
}

fn expand_sources(sources: &[String], cwd: &Path) -> Result<ExpandedSources> {
    let mut paths = Vec::new();
    let mut temp_dirs = Vec::new();
    for source in sources {
        if source == "local" {
            let live = db_path();
            if !live.is_file() {
                anyhow::bail!("live diary not found: {}", live.display());
            }
            paths.push(live);
            continue;
        }
        let path = resolve_source_path(source, cwd);
        if path.is_dir() {
            let packs = list_packs_in_dir(&path)?;
            for pack in packs {
                let temp = tempfile::Builder::new().prefix(PACK_PREFIX).tempdir()?;
                extract_pack_db(&pack, temp.path())?;
                paths.push(temp.path().join("events.db"));
                temp_dirs.push(temp);
            }
            continue;
        }
        if !is_tar_gz(&path) {
            anyhow::bail!("invalid pack: {}", path.display());
        }
        if !path.is_file() {
            anyhow::bail!("pack not found: {}", path.display());
        }
        let temp = tempfile::Builder::new().prefix(PACK_PREFIX).tempdir()?;
        extract_pack_db(&path, temp.path())?;
        paths.push(temp.path().join("events.db"));
        temp_dirs.push(temp);
    }
    Ok(ExpandedSources {
        paths,
        _temp_dirs: temp_dirs,
    })
}

fn resolve_source_path(source: &str, cwd: &Path) -> PathBuf {
    let path = Path::new(source);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn list_packs_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut packs = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_tar_gz(&path) {
            packs.push(path);
        }
    }
    packs.sort();
    Ok(packs)
}

pub(super) fn is_tar_gz(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "gz")
        && path
            .file_name()
            .and_then(|name| {
                name.to_str()
                    .and_then(|s| s.strip_suffix(".tar.gz"))
                    .map(|_| ())
            })
            .is_some()
}

fn extract_pack_db(pack: &Path, dest_dir: &Path) -> Result<()> {
    let file = File::open(pack).with_context(|| format!("open {}", pack.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut found = false;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path.file_name().is_some_and(|n| n == "events.db") {
            let out = dest_dir.join("events.db");
            entry.unpack(&out)?;
            found = true;
            break;
        }
    }
    if !found {
        anyhow::bail!("invalid pack: missing events.db in {}", pack.display());
    }
    Ok(())
}

pub(super) fn query_attached(
    paths: &[PathBuf],
    since: Option<&DateTime<Utc>>,
    until: Option<&DateTime<Utc>>,
    limit: Option<usize>,
    audience: Option<&str>,
    kind: Option<&str>,
    repo: Option<&str>,
) -> Result<Vec<Event>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_in_memory()?;
    for (idx, path) in paths.iter().enumerate() {
        let alias = format!("p{idx}");
        conn.execute(
            &format!("ATTACH DATABASE ?1 AS {alias}"),
            params![path.to_string_lossy().as_ref()],
        )?;
    }
    conn.pragma_update(None, "query_only", true)?;
    let union = paths
        .iter()
        .enumerate()
        .map(|(idx, _)| {
            let alias = format!("p{idx}");
            attached_events_select(&alias, attached_has_agent(&conn, &alias))
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let mut sql = format!(
        "SELECT id, ts, kind, via, audience, actor, repo, git_commit,
                command, command_truncated, hydrate_ms, matches, agent
         FROM ({union}) WHERE 1=1"
    );
    if since.is_some() {
        sql.push_str(" AND ts >= ?");
    }
    if until.is_some() {
        sql.push_str(" AND ts <= ?");
    }
    if audience.is_some() {
        sql.push_str(" AND audience = ?");
    }
    if kind.is_some() {
        sql.push_str(" AND kind = ?");
    }
    if repo.is_some() {
        sql.push_str(" AND (repo = ? OR repo = ?)");
    }
    sql.push_str(" GROUP BY id ORDER BY ts DESC");
    if limit.is_some() {
        sql.push_str(" LIMIT ?");
    }
    let mut stmt = conn.prepare(&sql)?;
    let mut idx = 1;
    if let Some(ts) = since {
        stmt.raw_bind_parameter(idx, ts.to_rfc3339_opts(SecondsFormat::Millis, true))?;
        idx += 1;
    }
    if let Some(ts) = until {
        stmt.raw_bind_parameter(idx, ts.to_rfc3339_opts(SecondsFormat::Millis, true))?;
        idx += 1;
    }
    if let Some(a) = audience {
        stmt.raw_bind_parameter(idx, a)?;
        idx += 1;
    }
    if let Some(k) = kind {
        stmt.raw_bind_parameter(idx, k)?;
        idx += 1;
    }
    if let Some(r) = repo {
        stmt.raw_bind_parameter(idx, r)?;
        idx += 1;
        stmt.raw_bind_parameter(idx, basename_or_self(r))?;
        idx += 1;
    }
    if let Some(n) = limit {
        stmt.raw_bind_parameter(idx, n as i64)?;
    }
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(event_from_row(row)?);
    }
    Ok(out)
}

fn attached_has_agent(conn: &Connection, alias: &str) -> bool {
    let sql = format!("PRAGMA {alias}.table_info(events)");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return false;
    };
    let Ok(mut rows) = stmt.query([]) else {
        return false;
    };
    while let Ok(Some(row)) = rows.next() {
        let Ok(name) = row.get::<_, String>(1) else {
            continue;
        };
        if name == "agent" {
            return true;
        }
    }
    false
}

fn attached_events_select(alias: &str, has_agent: bool) -> String {
    let agent = if has_agent {
        "json(agent) AS agent".to_string()
    } else {
        "NULL AS agent".to_string()
    };
    format!(
        "SELECT id, ts, kind, via, audience, actor, repo, git_commit,
                command, command_truncated, hydrate_ms, json(matches) AS matches, {agent}
         FROM {alias}.events"
    )
}
