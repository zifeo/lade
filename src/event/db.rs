use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use rusqlite_migration::{M, Migrations};
use serde_json::{Value, json};

use super::{
    BUSY_MS, EVENTS_AGENT_DDL, EVENTS_ARGV_DDL, EVENTS_DDL, Event, LogInfo, OPEN_ATTEMPTS,
    OPEN_RETRY_MS, db_path,
};

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(EVENTS_DDL),
        M::up(EVENTS_AGENT_DDL),
        M::up(EVENTS_ARGV_DDL),
    ])
}

fn map_migrate_err(err: rusqlite_migration::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(err.to_string())))
}

fn is_open_race(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(e, _) => matches!(
            e.code,
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
        ),
        rusqlite::Error::ToSqlConversionFailure(_) => true,
        _ => false,
    }
}

fn open_once(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_millis(BUSY_MS))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let migrations = migrations();
    if let Err(err) = migrations.to_latest(&mut conn) {
        // Peer already committed the schema. We read user_version=0
        // before their lock, then CREATE TABLE hit "already exists".
        if migrations.pending_migrations(&conn).ok() != Some(0) {
            return Err(map_migrate_err(err));
        }
    }
    Ok(conn)
}

pub fn open() -> rusqlite::Result<Connection> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    for attempt in 0..OPEN_ATTEMPTS {
        match open_once(&path) {
            Ok(conn) => return Ok(conn),
            Err(err) if attempt + 1 < OPEN_ATTEMPTS && is_open_race(&err) => {
                std::thread::sleep(Duration::from_millis(OPEN_RETRY_MS));
            }
            Err(err) => return Err(err),
        }
    }
    unreachable!("OPEN_ATTEMPTS is not zero")
}

pub fn query(
    since: Option<&DateTime<Utc>>,
    until: Option<&DateTime<Utc>>,
    limit: Option<usize>,
    audience: Option<&str>,
    kind: Option<&str>,
    repo: Option<&str>,
) -> rusqlite::Result<Vec<Event>> {
    let conn = open()?;
    let mut sql = String::from(
        "SELECT id, ts, kind, via, audience, actor, repo, git_commit,
                command, command_truncated, hydrate_ms, json(matches), json(agent),
                json(argv)
         FROM events WHERE 1=1",
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
        sql.push_str(" AND repo = ?");
    }
    sql.push_str(" ORDER BY ts DESC");
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
    }
    if let Some(n) = limit {
        stmt.raw_bind_parameter(idx, n as i64)?;
    }
    let rows = stmt.raw_query();
    let mut out = Vec::new();
    let mut rows = rows;
    while let Some(row) = rows.next()? {
        out.push(event_from_row(row)?);
    }
    Ok(out)
}

pub(crate) fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    let matches_raw: String = row.get(11)?;
    let matches = serde_json::from_str(&matches_raw).unwrap_or(json!([]));
    let agent = json_col(row, 12);
    let argv = json_col(row, 13);
    Ok(Event {
        id: row.get(0)?,
        ts: row.get(1)?,
        kind: row.get(2)?,
        via: row.get(3)?,
        audience: row.get(4)?,
        actor: row.get(5)?,
        repo: row.get(6)?,
        git_commit: row.get(7)?,
        command: row.get(8)?,
        command_truncated: row.get::<_, i64>(9)? != 0,
        argv,
        hydrate_ms: row.get(10)?,
        matches,
        agent,
    })
}

fn json_col(row: &rusqlite::Row<'_>, idx: usize) -> Option<Value> {
    row.get::<_, Option<String>>(idx)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(|value: &Value| !value.is_null())
}

pub fn prune_before(ts: &DateTime<Utc>) -> rusqlite::Result<usize> {
    let mut conn = open()?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let n = tx.execute(
        "DELETE FROM events WHERE ts < ?1",
        params![ts.to_rfc3339_opts(SecondsFormat::Millis, true)],
    )?;
    tx.commit()?;
    Ok(n)
}

pub fn warmup() {
    if let Err(err) = open() {
        log::debug!("lade event open failed: {err}");
    }
}

pub fn info() -> LogInfo {
    let path = db_path();
    if !path.is_file() {
        return LogInfo {
            path,
            events: 0,
            bytes: 0,
        };
    }
    let events = open()
        .ok()
        .and_then(|c| {
            c.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
                .optional()
                .ok()
                .flatten()
        })
        .unwrap_or(0);
    let mut bytes = 0u64;
    for suffix in ["", "-wal", "-shm"] {
        let p = PathBuf::from(format!("{}{suffix}", path.display()));
        if let Ok(meta) = std::fs::metadata(&p) {
            bytes += meta.len();
        }
    }
    LogInfo {
        path,
        events,
        bytes,
    }
}
