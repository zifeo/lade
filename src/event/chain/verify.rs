use rusqlite::{Connection, OptionalExtension, params};

use super::super::{EVENT_COLS, Event};
use super::{GENESIS, Head, insert_sealed, load_events, next_seal, read_head, seal, write_head};

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
                params![event.id, seq, prev_hash, row_hash, super::SEAL_VER],
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
    // One snapshot for the rows and the head: an append landing between the
    // two reads would look like a tampered head.
    let tx = conn.unchecked_transaction()?;
    verify_snapshot(&tx)
}

fn verify_snapshot(conn: &Connection) -> rusqlite::Result<ChainStatus> {
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
        let event = super::super::event_from_row(row)?;
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
