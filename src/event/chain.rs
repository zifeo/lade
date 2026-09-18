use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::Sha256;

use super::{EVENT_COLS, Event};

type HmacSha256 = Hmac<Sha256>;

/// Compile-time domain key. Same for every build of this line.
/// An editor without this binary cannot reseal a row. Anyone who
/// runs this lade can. That is tamper-evident, not tamper-proof.
const CHAIN_SALT: &[u8] = b"lade.events.chain.v1\x1f7c3e2a91b04d58e6";
const CHAIN_ALG: &[u8] = b"hmac-sha256-v1";
const SEAL_VER: &str = env!("CARGO_PKG_VERSION");
const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub(crate) const EVENTS_CHAIN_DDL: &str = "
        ALTER TABLE events ADD COLUMN seq INTEGER;
        ALTER TABLE events ADD COLUMN prev_hash TEXT;
        ALTER TABLE events ADD COLUMN row_hash TEXT;
        ALTER TABLE events ADD COLUMN seal_ver TEXT;
        CREATE UNIQUE INDEX events_seq ON events(seq);
        CREATE TABLE chain_head (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          head_seq INTEGER NOT NULL,
          head_hash TEXT NOT NULL,
          epoch INTEGER NOT NULL
        );
        CREATE TRIGGER events_no_delete BEFORE DELETE ON events BEGIN
          SELECT RAISE(ABORT, 'events are append-only');
        END;";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainBreak {
    pub seq: Option<i64>,
    pub ts: Option<String>,
    pub command: Option<String>,
    pub reason: String,
    pub before: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStatus {
    pub ok: bool,
    pub rows: usize,
    pub epoch: i64,
    pub breaks: Vec<ChainBreak>,
}

impl ChainStatus {
    pub fn report_lines(&self, path: impl std::fmt::Display) -> Vec<String> {
        if self.ok {
            return vec![format!("ok {path} rows={} epoch={}", self.rows, self.epoch)];
        }
        let path = path.to_string();
        self.breaks
            .iter()
            .map(|b| format_break(&path, b, self.rows))
            .collect()
    }
}

fn format_break(path: &str, b: &ChainBreak, total: usize) -> String {
    let seq = b.seq.map(|s| format!(" seq={s}")).unwrap_or_default();
    let where_ = match (&b.ts, &b.command) {
        (Some(ts), Some(cmd)) if !cmd.is_empty() => format!(" {ts} {cmd}"),
        (Some(ts), _) => format!(" {ts}"),
        _ => String::new(),
    };
    let after = match b.seq {
        Some(s) if s > 0 => total.saturating_sub(s as usize),
        _ => total.saturating_sub(b.before.saturating_add(1)),
    };
    format!(
        "tampered {path}{seq}{where_}: {}. {} before it. {} after it.",
        b.reason,
        count_label(b.before, "row"),
        count_label(after, "row")
    )
}

fn count_label(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

struct Head {
    seq: i64,
    hash: String,
    epoch: i64,
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn put(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    buf.extend_from_slice(bytes);
}

fn put_opt(buf: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(v) => put(buf, v.as_bytes()),
        None => put(buf, &[]),
    }
}

fn json_text(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

fn json_opt(value: &Option<serde_json::Value>) -> Option<String> {
    value
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "null".into()))
}

fn payload(event: &Event) -> Vec<u8> {
    let matches = json_text(&event.matches);
    let agent = json_opt(&event.agent);
    let argv = json_opt(&event.argv);
    let mut buf = Vec::new();
    put(&mut buf, event.id.as_bytes());
    put(&mut buf, event.ts.as_bytes());
    put(&mut buf, event.kind.as_bytes());
    put_opt(&mut buf, event.via.as_deref());
    put_opt(&mut buf, event.audience.as_deref());
    put_opt(&mut buf, event.actor.as_deref());
    put_opt(&mut buf, event.repo.as_deref());
    put_opt(&mut buf, event.git_commit.as_deref());
    put(&mut buf, event.command.as_bytes());
    put(&mut buf, &[u8::from(event.command_truncated)]);
    match event.hydrate_ms {
        Some(ms) => put(&mut buf, &ms.to_bits().to_be_bytes()),
        None => put(&mut buf, &[]),
    }
    put(&mut buf, matches.as_bytes());
    put_opt(&mut buf, agent.as_deref());
    put_opt(&mut buf, argv.as_deref());
    buf
}

fn seal(prev_hash: &str, event: &Event, seal_ver: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(CHAIN_SALT).expect("HMAC-SHA256 accepts any key length");
    mac.update(CHAIN_ALG);
    mac.update(&[0]);
    mac.update(seal_ver.as_bytes());
    mac.update(&[0]);
    mac.update(prev_hash.as_bytes());
    mac.update(&[0]);
    mac.update(&payload(event));
    hex_lower(&mac.finalize().into_bytes())
}

fn read_head(conn: &Connection) -> rusqlite::Result<Option<Head>> {
    conn.query_row(
        "SELECT head_seq, head_hash, epoch FROM chain_head WHERE id = 1",
        [],
        |row| {
            Ok(Head {
                seq: row.get(0)?,
                hash: row.get(1)?,
                epoch: row.get(2)?,
            })
        },
    )
    .optional()
}

fn write_head(tx: &Transaction<'_>, head: &Head) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO chain_head (id, head_seq, head_hash, epoch)
         VALUES (1, ?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET
           head_seq = excluded.head_seq,
           head_hash = excluded.head_hash,
           epoch = excluded.epoch",
        params![head.seq, head.hash, head.epoch],
    )?;
    Ok(())
}

fn next_seal(tx: &Transaction<'_>, event: &Event) -> rusqlite::Result<(i64, String, String, i64)> {
    let head = read_head(tx)?;
    let prev_hash = head
        .as_ref()
        .map(|h| h.hash.clone())
        .unwrap_or_else(|| GENESIS.to_string());
    let seq = head.as_ref().map(|h| h.seq + 1).unwrap_or(1);
    let epoch = head.as_ref().map(|h| h.epoch).unwrap_or(1);
    let row_hash = seal(&prev_hash, event, SEAL_VER);
    Ok((seq, prev_hash, row_hash, epoch))
}

fn load_events(
    tx: &Transaction<'_>,
    sql: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<Event>> {
    let mut stmt = tx.prepare(sql)?;
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(super::event_from_row(row)?);
    }
    Ok(out)
}

pub fn insert_sealed(tx: &Transaction<'_>, event: &Event) -> rusqlite::Result<()> {
    let (seq, prev_hash, row_hash, epoch) = next_seal(tx, event)?;
    let matches = json_text(&event.matches);
    let agent = json_opt(&event.agent);
    let argv = json_opt(&event.argv);
    tx.execute(
        "INSERT INTO events (
            id, ts, kind, via, audience, actor, repo, git_commit,
            command, command_truncated, hydrate_ms, matches, agent, argv,
            seq, prev_hash, row_hash, seal_ver
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, jsonb(?12), jsonb(?13), jsonb(?14),
                  ?15, ?16, ?17, ?18)",
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
            argv,
            seq,
            prev_hash,
            row_hash,
            SEAL_VER,
        ],
    )?;
    write_head(
        tx,
        &Head {
            seq,
            hash: row_hash,
            epoch,
        },
    )
}

pub fn is_head(tx: &Transaction<'_>, event_id: &str) -> rusqlite::Result<bool> {
    let Some(head) = read_head(tx)? else {
        return Ok(false);
    };
    let seq: Option<i64> = tx
        .query_row(
            "SELECT seq FROM events WHERE id = ?1",
            params![event_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(seq == Some(head.seq))
}

pub fn reseal_head(tx: &Transaction<'_>, event: &Event) -> rusqlite::Result<()> {
    let Some(head) = read_head(tx)? else {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    };
    let prev: String = tx.query_row(
        "SELECT prev_hash FROM events WHERE id = ?1 AND seq = ?2",
        params![event.id, head.seq],
        |row| row.get(0),
    )?;
    let row_hash = seal(&prev, event, SEAL_VER);
    let matches = json_text(&event.matches);
    let agent = json_opt(&event.agent);
    let argv = json_opt(&event.argv);
    let n = tx.execute(
        "UPDATE events SET
            command = ?2, command_truncated = ?3, hydrate_ms = ?4,
            matches = jsonb(?5), agent = jsonb(?6), argv = jsonb(?7),
            row_hash = ?8, seal_ver = ?9
         WHERE id = ?1 AND seq = ?10",
        params![
            event.id,
            event.command,
            event.command_truncated as i64,
            event.hydrate_ms,
            matches,
            agent,
            argv,
            row_hash,
            SEAL_VER,
            head.seq,
        ],
    )?;
    if n != 1 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    write_head(
        tx,
        &Head {
            seq: head.seq,
            hash: row_hash,
            epoch: head.epoch,
        },
    )
}

pub fn backfill(conn: &mut Connection) -> rusqlite::Result<()> {
    let needs_seal: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM events WHERE row_hash IS NULL)",
        [],
        |row| row.get(0),
    )?;
    if needs_seal {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let pending = load_events(
            &tx,
            &format!(
                "SELECT {EVENT_COLS} FROM events WHERE row_hash IS NULL ORDER BY ts ASC, id ASC"
            ),
            [],
        )?;
        for event in pending {
            let (seq, prev_hash, row_hash, epoch) = next_seal(&tx, &event)?;
            tx.execute(
                "UPDATE events SET seq = ?2, prev_hash = ?3, row_hash = ?4, seal_ver = ?5
                 WHERE id = ?1 AND row_hash IS NULL",
                params![event.id, seq, prev_hash, row_hash, SEAL_VER],
            )?;
            write_head(
                &tx,
                &Head {
                    seq,
                    hash: row_hash,
                    epoch,
                },
            )?;
        }
        return tx.commit();
    }
    if read_head(conn)?.is_some() {
        return Ok(());
    }
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
    if count == 0 {
        return Ok(());
    }
    let tx = conn.transaction()?;
    if let Some((seq, hash)) = tx
        .query_row(
            "SELECT seq, row_hash FROM events WHERE seq IS NOT NULL ORDER BY seq DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
    {
        write_head(
            &tx,
            &Head {
                seq,
                hash,
                epoch: 1,
            },
        )?;
    }
    tx.commit()
}

pub fn prune_and_reseal(
    conn: &mut Connection,
    cutoff: &str,
    repo: Option<&str>,
) -> rusqlite::Result<usize> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let prev_epoch = read_head(&tx)?.map(|h| h.epoch).unwrap_or(1);
    tx.execute("DROP TRIGGER IF EXISTS events_no_delete", [])?;
    let kept = match repo {
        Some(repo) => load_events(
            &tx,
            &format!(
                "SELECT {EVENT_COLS} FROM events
                 WHERE NOT (ts < ?1 AND repo = ?2)
                 ORDER BY ts ASC, id ASC"
            ),
            params![cutoff, repo],
        )?,
        None => load_events(
            &tx,
            &format!("SELECT {EVENT_COLS} FROM events WHERE ts >= ?1 ORDER BY ts ASC, id ASC"),
            params![cutoff],
        )?,
    };
    let before: i64 = tx.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
    tx.execute("DELETE FROM events", [])?;
    tx.execute("DELETE FROM chain_head", [])?;
    for event in &kept {
        insert_sealed(&tx, event)?;
    }
    let mut head = read_head(&tx)?.unwrap_or(Head {
        seq: 0,
        hash: GENESIS.to_string(),
        epoch: 0,
    });
    head.epoch = prev_epoch + 1;
    write_head(&tx, &head)?;
    tx.execute(
        "CREATE TRIGGER events_no_delete BEFORE DELETE ON events BEGIN
           SELECT RAISE(ABORT, 'events are append-only');
         END",
        [],
    )?;
    tx.commit()?;
    Ok((before as usize).saturating_sub(kept.len()))
}

pub fn verify(conn: &Connection) -> rusqlite::Result<ChainStatus> {
    let epoch = read_head(conn)?.map(|h| h.epoch).unwrap_or(0);
    let mut stmt = conn.prepare(&format!(
        "SELECT {EVENT_COLS}, seq, prev_hash, row_hash, seal_ver
         FROM events ORDER BY seq ASC"
    ))?;
    let mut rows = stmt.query([])?;
    let mut prev = GENESIS.to_string();
    let mut expect_seq = 1i64;
    let mut count = 0usize;
    let mut intact = 0usize;
    let mut breaks = Vec::new();
    while let Some(row) = rows.next()? {
        let event = super::event_from_row(row)?;
        let seq: Option<i64> = row.get(14)?;
        let prev_hash: Option<String> = row.get(15)?;
        let row_hash: Option<String> = row.get(16)?;
        let seal_ver: Option<String> = row.get(17)?;
        count += 1;
        match (seq, prev_hash, row_hash, seal_ver) {
            (Some(seq), Some(stored_prev), Some(stored_hash), Some(seal_ver)) => {
                let mut reasons = Vec::new();
                if seq != expect_seq {
                    reasons.push("seq gap");
                }
                if stored_prev != prev {
                    reasons.push("prev_hash does not match the previous row");
                }
                if seal(&stored_prev, &event, &seal_ver) != stored_hash {
                    reasons.push("row_hash does not match the sealed payload");
                }
                if reasons.is_empty() {
                    intact += 1;
                } else {
                    breaks.push(break_at(&event, Some(seq), &reasons.join("; "), intact));
                }
                prev = stored_hash;
                expect_seq = seq + 1;
            }
            (seq, prev_hash, row_hash, seal_ver) => {
                let (marked, reason) = missing_seal(seq, &prev_hash, &row_hash, &seal_ver);
                breaks.push(break_at(&event, marked, reason, intact));
                if let Some(s) = marked {
                    expect_seq = s + 1;
                }
            }
        }
    }
    if let Some(head) = read_head(conn)? {
        if count == 0 && head.seq != 0 {
            breaks.push(ChainBreak {
                seq: None,
                ts: None,
                command: None,
                reason: "chain_head points at a missing row".into(),
                before: 0,
            });
        } else if count > 0 && head.hash != prev {
            breaks.push(ChainBreak {
                seq: Some(head.seq),
                ts: None,
                command: None,
                reason: "chain_head does not match the last row".into(),
                before: intact,
            });
        }
    }
    Ok(ChainStatus {
        ok: breaks.is_empty(),
        rows: count,
        epoch,
        breaks,
    })
}

fn missing_seal(
    seq: Option<i64>,
    prev_hash: &Option<String>,
    row_hash: &Option<String>,
    seal_ver: &Option<String>,
) -> (Option<i64>, &'static str) {
    if seq.is_none() {
        (None, "row has no seq")
    } else if prev_hash.is_none() {
        (seq, "row has no prev_hash")
    } else if row_hash.is_none() {
        (seq, "row has no row_hash")
    } else if seal_ver.is_none() {
        (seq, "row has no seal_ver")
    } else {
        (seq, "row has no seq")
    }
}

fn break_at(event: &Event, seq: Option<i64>, reason: &str, before: usize) -> ChainBreak {
    ChainBreak {
        seq,
        ts: Some(event.ts.clone()),
        command: Some(event.command.clone()),
        reason: reason.to_string(),
        before,
    }
}

pub fn unsigned_pack() -> ChainStatus {
    ChainStatus {
        ok: false,
        rows: 0,
        epoch: 0,
        breaks: vec![ChainBreak {
            seq: None,
            ts: None,
            command: None,
            reason: "no chain".into(),
            before: 0,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(id: &str, command: &str) -> Event {
        Event {
            id: id.into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind: "seen".into(),
            via: Some("organic".into()),
            audience: Some("human".into()),
            actor: None,
            repo: None,
            git_commit: None,
            command: command.into(),
            command_truncated: false,
            argv: None,
            hydrate_ms: None,
            matches: json!([]),
            agent: None,
        }
    }

    #[test]
    fn edit_breaks_the_chain() {
        let a = ev("1", "echo");
        let hash = seal(GENESIS, &a, "0.0.0");
        let mut b = a.clone();
        b.command = "rm".into();
        assert_ne!(seal(GENESIS, &b, "0.0.0"), hash);
    }

    #[test]
    fn next_row_binds_prev() {
        let a = ev("1", "echo");
        let h1 = seal(GENESIS, &a, "0.0.0");
        let b = ev("2", "ls");
        let h2 = seal(&h1, &b, "0.0.0");
        assert_ne!(h1, h2);
        assert_ne!(seal(GENESIS, &b, "0.0.0"), h2);
    }
}
