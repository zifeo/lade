use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TicketSecret {
    // § ezvgtge5j.b@7k37f
    pub key: String,
    pub source: String,
    pub private: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<PathBuf>,
    pub cwd: PathBuf,
    /// Per-rule `1password_service_account` URI. Last-wins overlay still
    /// applies per key; ticket replay must not reuse one SA for every secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_sa: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TicketNetwork {
    pub key: String,
    pub uri: String,
    /// Rule body `ttl:` as [`crate::config::RuleTtl::to_raw`] writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreEvent {
    pub command: String,
    pub cwd: PathBuf,
    pub via: String,
    pub audience: String,
    pub actor: Option<String>,
    pub log: bool,
    pub disclaimers: Vec<String>,
    pub secrets: Vec<TicketSecret>,
    pub network: Vec<TicketNetwork>,
    pub matches: Value,
    /// Free-form hook metadata. Absent on older tickets.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub agent: Value,
    /// Set when preexec withheld a disclaimer. `lade approve` requires this.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

impl PreEvent {
    /// The fields every surface agrees on, before any provider work is
    /// attached.
    pub(crate) fn new(
        command: &str,
        cwd: PathBuf,
        via: crate::audience::Via,
        audience: crate::config::Audience,
        actor: Option<String>,
    ) -> Self {
        Self {
            command: command.to_string(),
            cwd,
            via: via.child_stamp().unwrap_or("unknown").to_string(),
            audience: audience.as_str().to_string(),
            actor,
            log: false,
            disclaimers: Vec::new(),
            secrets: Vec::new(),
            network: Vec::new(),
            matches: Value::Array(Vec::new()),
            agent: Value::Null,
            pending: false,
        }
    }
}

pub const RESERVED: &[&str] = &["eval", "help", "hook", "user"];

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

pub fn is_id(s: &str) -> bool {
    // § ac9463jhq.b@t1rbm
    s.len() == 4 && s.bytes().all(|b| b.is_ascii_alphanumeric()) && !RESERVED.contains(&s)
}

/// 4 random A-Za-z0-9 chars, not reserved.
/// Use the UUID random tail, not the v7 timestamp prefix. Two writes in the
/// same millisecond must not emit the same id or `create_new` spins.
pub fn new_id() -> String {
    id_from_draws(|| {
        let uuid = uuid::Uuid::now_v7();
        let bytes = uuid.as_bytes();
        [bytes[8], bytes[9], bytes[10], bytes[11]]
    })
}

/// `draw` gives the random bytes of each try, so a test can draw a verb.
fn id_from_draws(mut draw: impl FnMut() -> [u8; 4]) -> String {
    // § ac9463jhq.b@5217c
    loop {
        let id: String = draw()
            .iter()
            .map(|byte| CHARSET[*byte as usize % CHARSET.len()] as char)
            .collect();
        if is_id(&id) {
            return id;
        }
    }
}

pub fn dir() -> PathBuf {
    // Test override, same reason as LADE_CONFIG_PATH: do not poison TMPDIR.
    if let Ok(path) = std::env::var("LADE_TICKET_DIR")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    crate::cache::tickets()
}

pub fn path(id: &str) -> PathBuf {
    dir().join(format!("{id}.json"))
}

const WRITE_TRIES: u32 = 16;

/// create_new the file. Retry new_id if exists. No fsync. Return id.
pub fn write(pre: &PreEvent) -> Result<String> {
    write_drawing(pre, new_id)
}

/// `draw` gives the ids to try, so a test can draw one that exists.
fn write_drawing(pre: &PreEvent, mut draw: impl FnMut() -> String) -> Result<String> {
    // § ac9463jhq.c@gmmny
    let json = serde_json::to_vec(pre).context("failed to serialize pre-event")?;
    for _ in 0..WRITE_TRIES {
        fs::create_dir_all(dir()).context("failed to create ticket directory")?;
        let id = draw();
        let file_path = path(&id);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file_path)
        {
            Ok(mut file) => {
                file.write_all(&json)
                    .with_context(|| format!("failed to write ticket {}", file_path.display()))?;
                return Ok(id);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("failed to create ticket {}", file_path.display()));
            }
        }
    }
    anyhow::bail!("could not create a ticket after {WRITE_TRIES} tries")
}

pub fn read(id: &str) -> Result<PreEvent> {
    let file_path = path(id);
    let bytes = fs::read(&file_path)
        .with_context(|| format!("failed to read ticket {}", file_path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse ticket {}", file_path.display()))
}

/// Pre-exec provider pids in a ticket that a Lade before hub tunnels wrote.
/// That Lade's `lade unset` stopped them; after an upgrade mid-command,
/// this one must.
pub fn legacy_network_pids(id: &str) -> Vec<u32> {
    #[derive(Deserialize)]
    struct Legacy {
        #[serde(default)]
        network_pids: Vec<u32>,
    }
    fs::read(path(id))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Legacy>(&bytes).ok())
        .map_or_else(Vec::new, |legacy| legacy.network_pids)
}

/// Missing file is Ok.
/// A pre-tool ticket nobody read within this long never will be: the
/// harness denied or dropped its command, or the line never ran Lade.
// § one gy20cgb72.d@183eb
pub const PRETOOL_UNREAD: std::time::Duration = std::time::Duration::from_secs(3600);

/// Deletes pre-tool tickets older than `age`. A wrap that comes later
/// still runs: it reads the yaml instead. Pre-exec tickets stay, since
/// `lade unset` needs theirs when a long command ends.
pub fn sweep_pretool(age: std::time::Duration) {
    // § gy20cgb72.d@zp7e0
    let Ok(entries) = fs::read_dir(dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| modified.elapsed().is_ok_and(|elapsed| elapsed > age));
        if !stale || path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        // A ticket that does not parse is of no use to any reader.
        let pretool = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PreEvent>(&bytes).ok())
            .is_none_or(|pre| pre.via == crate::audience::Via::PRETOOL);
        if pretool {
            let _ = fs::remove_file(&path);
        }
    }
}

pub fn exists(id: &str) -> bool {
    // § qnn5xnx4z@dtxdy
    // `peel_pretool` runs before the startup check of the cache root, which
    // then stops Lade with its box: no root, no ticket.
    is_id(id) && crate::cache::checked_root().is_ok() && path(id).exists()
}

/// Overwrite an existing id. Used to add pids or flip `pending`.
pub fn replace(id: &str, pre: &PreEvent) -> Result<()> {
    if !is_id(id) {
        anyhow::bail!("invalid ticket id");
    }
    let json = serde_json::to_vec(pre).context("failed to serialize pre-event")?;
    let file_path = path(id);
    crate::cache::write_if_changed(&file_path, &json)
        .with_context(|| format!("failed to write ticket {}", file_path.display()))?;
    Ok(())
}

pub fn unlink(id: &str) -> Result<()> {
    let file_path = path(id);
    match fs::remove_file(&file_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            Err(e).with_context(|| format!("failed to unlink ticket {}", file_path.display()))
        }
    }
}

/// Peel `--pretool` optional 4-char id from argv. Leave `--pretool` itself
/// so clap's bool still works.
/// `--pretool=x7Km` -> id Some, argv keeps `--pretool` only.
/// `--pretool x7Km` (is_id) -> consume x7Km, keep `--pretool`.
/// `--pretool inject` -> do not consume inject.
/// Only Lade's own options are read: from `--` or the command's first word
/// on, `--pretool` belongs to the command.
pub fn peel_pretool(args: Vec<OsString>) -> (Option<String>, Vec<OsString>) {
    // § ac9463jhq.a@y4ept
    let mut ticket_id: Option<String> = None;
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let arg_str = arg.to_string_lossy();
        let command_starts = arg_str == "--" || (!arg_str.starts_with('-') && arg_str != "inject");
        if i > 0 && command_starts {
            out.extend(args[i..].iter().cloned());
            break;
        }
        if arg_str == "--pretool" {
            out.push(arg.clone());
            if i + 1 < args.len() {
                let next = args[i + 1].to_string_lossy();
                // Space form shares the slot with commands (`echo`, `yarn`).
                // Only peel when the ticket file is already there.
                if exists(&next) {
                    ticket_id = Some(next.into_owned());
                    i += 2;
                    continue;
                }
            }
            i += 1;
        } else if let Some(rest) = arg_str.strip_prefix("--pretool=") {
            if is_id(rest) {
                ticket_id = Some(rest.to_string());
            }
            out.push(OsString::from("--pretool"));
            i += 1;
        } else {
            out.push(arg.clone());
            i += 1;
        }
    }
    (ticket_id, out)
}

#[cfg(test)]
mod tests;
