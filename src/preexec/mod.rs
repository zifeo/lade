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
