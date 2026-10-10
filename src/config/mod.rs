mod collect;
mod hydrate;
mod loader;
mod patterns;
mod plan;
mod query;
mod resolve;
mod secret;
#[cfg(test)]
mod tests;
mod ttl;
mod walk;

pub use loader::LadeFile;
pub(crate) use loader::{
    at_user_home, config_in_dir, parse_lade_yaml, render_lade_yaml, report_load_error,
    require_lade_version, yaml_files_on_walk,
};
pub(crate) use resolve::{ResolvedEntry, binding_name, resolve_entry};
pub use secret::*;
pub(crate) use ttl::{body_put_ttl_ms, is_shell_uri, uri_is_cacheable};
pub(crate) use walk::hash_yaml_files;

use crate::global_config::GlobalConfig;
use crate::ticket::TicketSecret;
use anyhow::Result;
use patterns::CompiledPatterns;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct SecretSources {
    pub sources: HashMap<String, String>,
    pub overridden: HashSet<String>,
    pub cancelled: HashMap<String, String>,
    pub silent: HashSet<String>,
}

pub type Output = Option<PathBuf>;

/// Who secrets are for. Produced by [`crate::audience::detect`], never picked
/// by callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Human,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkBinding {
    pub key: String,
    pub uri: String,
}

/// Pre-event payload built from already-matched rules. Secret values are
/// sources/URIs only. [`Self::log`] is last explicit `log` on those matches.
/// No-match `seen` uses [`Config::log_on_walk`].
pub(crate) struct PreEventWork {
    pub disclaimers: Vec<String>,
    pub secrets: Vec<TicketSecret>,
    pub network: Vec<NetworkBinding>,
    pub matches: Value,
    pub op_sa: Option<String>,
    pub log: bool,
    pub progress: SecretSources,
}

impl PreEventWork {
    /// Secrets, tunnels, or a disclaimer. `log` / `when` / `silence` alone
    /// do not need a wrap.
    pub(crate) fn needs_inject(&self) -> bool {
        !self.secrets.is_empty() || !self.network.is_empty() || !self.disclaimers.is_empty()
    }
}

pub struct Config {
    rules: Vec<(PathBuf, LadeRule)>,
    patterns: Vec<String>,
    compiled: CompiledPatterns,
    walk_hash: [u8; 32],
}

impl Config {
    pub(crate) fn new(
        rules: Vec<(PathBuf, LadeRule)>,
        patterns: Vec<String>,
        compiled: CompiledPatterns,
        walk_hash: [u8; 32],
    ) -> Self {
        Config {
            rules,
            patterns,
            compiled,
            walk_hash,
        }
    }

    pub(crate) fn walk_hash(&self) -> [u8; 32] {
        self.walk_hash
    }

    /// Loaded rules in overlay order, with the file directory and pattern.
    pub(crate) fn rule_entries(&self) -> impl Iterator<Item = (&PathBuf, &str, &LadeRule)> {
        self.rules
            .iter()
            .zip(self.patterns.iter())
            .map(|((path, rule), pattern)| (path, pattern.as_str(), rule))
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// The configured user (global config override, falling back to the OS
/// user), used to resolve per-user secret/network maps. Reads
/// [`GlobalConfig`] from disk, so callers on the hot path (one shell command
/// = one invocation) should resolve it once and pass it down rather than
/// calling this repeatedly.
pub(crate) async fn saved_user() -> Result<Option<String>> {
    let local_config = GlobalConfig::load().await?;
    Ok(local_config.user.or_else(GlobalConfig::os_user))
}

pub(crate) fn is_valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().enumerate().all(|(idx, ch)| {
            if idx == 0 {
                ch == '_' || ch.is_ascii_alphabetic()
            } else {
                ch == '_' || ch.is_ascii_alphanumeric()
            }
        })
}
