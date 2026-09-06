mod collect;
mod hydrate;
mod loader;
mod plan;
mod secret;
#[cfg(test)]
mod tests;

pub use loader::LadeFile;
pub use secret::*;

use crate::global_config::GlobalConfig;
use crate::ticket::TicketSecret;
use anyhow::Result;
use regex::RegexSet;
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

pub struct Config {
    rules: Vec<(PathBuf, LadeRule)>,
    patterns: Vec<String>,
    regex_set: RegexSet,
}

impl Config {
    pub(crate) fn new(
        rules: Vec<(PathBuf, LadeRule)>,
        patterns: Vec<String>,
        regex_set: RegexSet,
    ) -> Self {
        Config {
            rules,
            patterns,
            regex_set,
        }
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
    use std::env;

    let local_config = GlobalConfig::load().await?;
    Ok(local_config
        .user
        .or_else(|| env::var("USER").ok().or_else(|| env::var("USERNAME").ok())))
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
