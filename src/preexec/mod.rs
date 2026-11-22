mod hook;
mod profile;
mod shell;

pub use hook::{
    SetupShell, ci_job, enable_current_preexec, maybe_bootstrap_setup_shell, preexec_installed,
    present_shells, uninstall_current_preexec,
};
pub(crate) use hook::{reload_after_upgrade_lines, wrap_reload_command};
pub(crate) use profile::path_for_display;
pub use shell::Shell;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;

pub const LADE_APPROVE: &str = "LADE_APPROVE";
pub const LADE_RESTORE: &str = "LADE_RESTORE";
pub const LADE_VIA: &str = "LADE_VIA";
pub const LADE_T: &str = "LADE_T";

/// Protocol env the child must not inherit. `LADE_APPROVE` stays.
pub const CHILD_UNSET: [&str; 3] = [LADE_VIA, LADE_T, LADE_RESTORE];

/// The `LADE_APPROVE` this process started with, once `lade approve` set its
/// own to let its command through.
static APPROVE_BEFORE: std::sync::OnceLock<Option<std::ffi::OsString>> = std::sync::OnceLock::new();

/// Lets this process's disclaimers through with `code`.
pub fn approve_in_process(code: &str) {
    let _ = APPROVE_BEFORE.set(std::env::var_os(LADE_APPROVE));
    unsafe {
        std::env::set_var(LADE_APPROVE, code);
    }
}

/// The child gets the `LADE_APPROVE` Lade started with, not the one
/// `lade approve` set for itself.
pub fn restore_approve(cmd: &mut std::process::Command) {
    match APPROVE_BEFORE.get() {
        Some(Some(value)) => {
            cmd.env(LADE_APPROVE, value);
        }
        Some(None) => {
            cmd.env_remove(LADE_APPROVE);
        }
        None => {}
    }
}

pub fn strip_child_protocol(cmd: &mut std::process::Command) {
    for key in CHILD_UNSET {
        cmd.env_remove(key);
    }
}

pub fn strip_child_protocol_tokio(cmd: &mut tokio::process::Command) {
    for key in CHILD_UNSET {
        cmd.env_remove(key);
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestorePayload {
    /// The `LADE_T` of the line that took the snapshot. A nested shell
    /// inherits the outer line's snapshot, which is not its own to restore.
    pub ticket: String,
    pub env: HashMap<String, Option<String>>,
}

impl RestorePayload {
    pub fn encode(&self) -> Result<String> {
        encode_v1("LADE_RESTORE", self)
    }

    pub fn decode(value: &str) -> Result<Self> {
        decode_v1("LADE_RESTORE", value)
    }
}

fn encode_v1<T: Serialize>(label: &str, value: &T) -> Result<String> {
    let json = serde_json::to_string(value).with_context(|| format!("failed to encode {label}"))?;
    Ok(format!("v1:{}", URL_SAFE_NO_PAD.encode(json)))
}

fn decode_v1<T: DeserializeOwned>(label: &str, value: &str) -> Result<T> {
    let encoded = value
        .strip_prefix("v1:")
        .with_context(|| format!("invalid or unsupported {label} version"))?;
    let json = URL_SAFE_NO_PAD
        .decode(encoded)
        .with_context(|| format!("failed to decode {label} base64"))?;
    serde_json::from_slice(&json).with_context(|| format!("failed to parse {label} JSON"))
}

#[cfg(test)]
mod tests;
