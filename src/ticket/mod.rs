use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TicketSecret {
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
    /// Resolved 1Password service-account URI, not the token. Absent from the
    /// protocol table; wrap still needs it to hydrate without YAML.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_sa: Option<String>,
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
            op_sa: None,
            agent: Value::Null,
            pending: false,
        }
    }
}

pub const RESERVED: &[&str] = &["eval", "help", "hook", "user"];

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

pub fn is_id(s: &str) -> bool {
    s.len() == 4 && s.bytes().all(|b| b.is_ascii_alphanumeric()) && !RESERVED.contains(&s)
}

/// 4 random A-Za-z0-9 chars, not reserved.
/// Use the UUID random tail, not the v7 timestamp prefix. Two writes in the
/// same millisecond must not emit the same id or `create_new` spins.
pub fn new_id() -> String {
    loop {
        let bytes = uuid::Uuid::now_v7();
        let bytes = bytes.as_bytes();
        let id: String = (0..4)
            .map(|i| CHARSET[(bytes[8 + i] as usize) % CHARSET.len()] as char)
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
    let json = serde_json::to_vec(pre).context("failed to serialize pre-event")?;
    for _ in 0..WRITE_TRIES {
        fs::create_dir_all(dir()).context("failed to create ticket directory")?;
        let id = new_id();
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
pub fn exists(id: &str) -> bool {
    is_id(id) && path(id).exists()
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

/// Keep `existing` when that file is already there. Otherwise `write`.
pub fn write_or_replace(existing: Option<&str>, pre: &PreEvent) -> Result<String> {
    if let Some(id) = existing.filter(|id| exists(id)) {
        replace(id, pre)?;
        return Ok(id.to_string());
    }
    write(pre)
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
pub fn peel_pretool(args: Vec<OsString>) -> (Option<String>, Vec<OsString>) {
    let mut ticket_id: Option<String> = None;
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let arg_str = arg.to_string_lossy();
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
