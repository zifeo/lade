mod loader;
mod secret;
#[cfg(test)]
mod tests;

pub use loader::LadeFile;
use secret::resolve_lade_secret;
pub use secret::*;

use crate::global_config::GlobalConfig;
use crate::provider_registry::is_network_scheme;
use anyhow::{Result, bail};
use futures::future::try_join_all;
use lade_sdk::{hydrate_one, hydrate_with_maskable};
use regex::RegexSet;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::{collections::HashMap, path::PathBuf};

pub type Output = Option<PathBuf>;

type VarsByOutput = FxHashMap<Output, HashMap<String, String>>;

type CollectHydrateAccum = (
    VarsByOutput,
    HashMap<String, String>,
    FxHashSet<String>,
    Vec<String>,
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkBinding {
    pub key: String,
    pub uri: String,
}

/// A single rule entry, resolved for a user and classified as either a plain
/// secret/file value or a network provider binding (kubectl://, kubefwd://,
/// tsh://). Centralizing this classification keeps the scheme/numeric-key
/// rules consistent across hydration, `unset`, and network binding
/// collection, instead of each call site re-deriving them slightly
/// differently.
enum ResolvedEntry {
    Secret {
        key: String,
        value: String,
    },
    Network {
        key: String,
        uri: String,
    },
    /// A numeric key (port number) resolved to a non-network value. Only
    /// `rule_sources`/`network_bindings_from_rules` treat this as an error;
    /// `keys_from_rules` (used for `unset`) just skips it, since by the time
    /// `unset` runs, `set`/`inject` would already have failed on it.
    InvalidNumericSecret {
        key: String,
    },
}

fn resolve_entry(
    key: &str,
    secret: &LadeSecret,
    saved_user: &Option<String>,
) -> Option<ResolvedEntry> {
    let value = resolve_lade_secret(secret, saved_user)?;
    if split_scheme(&value).is_some_and(is_network_scheme) {
        return Some(ResolvedEntry::Network {
            key: key.to_string(),
            uri: value,
        });
    }
    if key.parse::<u16>().is_ok() {
        return Some(ResolvedEntry::InvalidNumericSecret {
            key: key.to_string(),
        });
    }
    Some(ResolvedEntry::Secret {
        key: key.to_string(),
        value,
    })
}

fn output_name(output: &Output) -> String {
    output
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "environment".to_string())
}

fn merge_vars(
    vars: &mut VarsByOutput,
    output: Output,
    incoming: HashMap<String, String>,
) -> Result<()> {
    let target = vars.entry(output.clone()).or_default();
    for (key, value) in incoming {
        match target.get(&key) {
            Some(existing) if existing != &value => bail!(
                "conflicting value for '{}' in {}: '{}' and '{}' match the same command; use more specific rules",
                key,
                output_name(&output),
                existing,
                value
            ),
            Some(_) => {}
            None => {
                target.insert(key, value);
            }
        }
    }
    Ok(())
}

fn merge_sources(
    sources: &mut HashMap<String, String>,
    incoming: HashMap<String, String>,
) -> Result<()> {
    for (key, source) in incoming {
        match sources.get(&key) {
            Some(existing) if existing != &source => bail!(
                "conflicting source for '{}': '{}' and '{}' match the same command; use one source per variable",
                key,
                existing,
                source
            ),
            Some(_) => {}
            None => {
                sources.insert(key, source);
            }
        }
    }
    Ok(())
}

pub fn split_scheme(value: &str) -> Option<&str> {
    value.split_once("://").map(|(scheme, _)| scheme)
}

/// Secret values only (no network bindings) for a single rule, keyed by
/// name. Used for hydration, so it applies to both env and file-routed
/// outputs alike — unlike [`Config::keys_from_rules`], it does not require
/// keys to look like valid env var names (a file-routed secret can use any
/// key as its JSON/YAML field name).
fn rule_sources(rule: &LadeRule, saved_user: &Option<String>) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for (key, secret) in &rule.secrets {
        match resolve_entry(key, secret, saved_user) {
            Some(ResolvedEntry::Secret { key, value }) => {
                out.insert(key, value);
            }
            Some(ResolvedEntry::Network { .. }) | None => {}
            Some(ResolvedEntry::InvalidNumericSecret { key }) => bail!(
                "numeric key '{}' must use a network URI (kubectl://, kubefwd://, tsh://)",
                key
            ),
        }
    }
    Ok(out)
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

pub struct Config {
    rules: Vec<(PathBuf, LadeRule)>,
    regex_set: RegexSet,
}

impl Config {
    pub(crate) fn new(rules: Vec<(PathBuf, LadeRule)>, regex_set: RegexSet) -> Self {
        Config { rules, regex_set }
    }

    /// Rules matching `command`. Synchronous and I/O-free: cloning the small
    /// number of matched rules out of `self.rules`. Callers on the hot path
    /// should call this once per invocation and reuse the result, rather
    /// than letting each downstream step (disclaimers, network bindings,
    /// secret sources, hydration) re-match independently.
    pub(crate) fn collect(&self, command: &str) -> Vec<(PathBuf, LadeRule)> {
        self.regex_set
            .matches(command)
            .into_iter()
            .map(|i| self.rules[i].clone())
            .collect()
    }

    async fn hydrate_output(
        &self,
        path: PathBuf,
        rule: LadeRule,
        saved_user: &Option<String>,
    ) -> Result<(
        Output,
        HashMap<String, String>,
        HashMap<String, String>,
        FxHashSet<String>,
        Vec<String>,
    )> {
        let sources = rule_sources(&rule, saved_user)?;

        let config = rule.config.as_ref();
        let output = config.and_then(|c| c.file.clone());
        let extra_env = if let Some(uri) = config
            .and_then(|c| c.onepassword_service_account.as_ref())
            .and_then(|sa| resolve_lade_secret(sa, saved_user))
        {
            let token = hydrate_one(uri, &path, &HashMap::new()).await?;
            HashMap::from([("OP_SERVICE_ACCOUNT_TOKEN".to_string(), token)])
        } else {
            HashMap::new()
        };

        let (values, maskable, warnings) =
            hydrate_with_maskable(sources.clone(), path.clone(), extra_env).await?;
        Ok((
            output.map(|subpath| path.join(subpath)),
            values,
            sources,
            maskable,
            warnings,
        ))
    }

    /// Hydrate already-collected `rules` against an already-resolved
    /// `saved_user`. Hot-path callers (`run_inject`/`handle_set`) should use
    /// this directly with the single `collect`+`saved_user` resolved at the
    /// top of the invocation, instead of [`Config::collect_hydrate`] which
    /// re-resolves both.
    pub async fn hydrate_rules(
        &self,
        rules: &[(PathBuf, LadeRule)],
        saved_user: &Option<String>,
    ) -> Result<(
        HashMap<Output, HashMap<String, String>>,
        HashMap<String, String>,
        FxHashSet<String>,
        Vec<String>,
    )> {
        let (vars, sources, maskable, warnings): CollectHydrateAccum = try_join_all(
            rules
                .iter()
                .cloned()
                .map(|(path, rule)| self.hydrate_output(path, rule, saved_user)),
        )
        .await?
        .into_iter()
        .try_fold(
            (
                FxHashMap::default(),
                HashMap::new(),
                FxHashSet::default(),
                Vec::new(),
            ),
            |(mut vars, mut sources, mut maskable, mut warnings),
             (output, map, rule_sources, rule_maskable, rule_warnings)| {
                merge_vars(&mut vars, output, map)?;
                merge_sources(&mut sources, rule_sources)?;
                maskable.extend(rule_maskable);
                warnings.extend(rule_warnings);
                Ok::<_, anyhow::Error>((vars, sources, maskable, warnings))
            },
        )?;
        Ok((vars.into_iter().collect(), sources, maskable, warnings))
    }

    #[cfg(test)]
    pub async fn collect_hydrate(
        &self,
        command: &str,
    ) -> Result<(
        HashMap<Output, HashMap<String, String>>,
        HashMap<String, String>,
        FxHashSet<String>,
        Vec<String>,
    )> {
        let saved_user = saved_user().await?;
        self.hydrate_rules(&self.collect(command), &saved_user)
            .await
    }

    /// Secret sources only (no hydration) for already-collected `rules`.
    /// Used to display provider progress groups before hydration runs.
    pub fn secret_sources_from_rules(
        rules: &[(PathBuf, LadeRule)],
        saved_user: &Option<String>,
    ) -> Result<HashMap<String, String>> {
        let mut sources = HashMap::new();
        for (_, rule) in rules {
            merge_sources(&mut sources, rule_sources(rule, saved_user)?)?;
        }
        Ok(sources)
    }

    /// Env var names per [`Output`] for already-collected `rules`, used to
    /// build the `unset` command. Unlike [`rule_sources`], this only keeps
    /// secrets whose key is a valid env var name (file-routed entries don't
    /// need that, but only the env-routed ones are ever turned into a shell
    /// `unset VAR` — see [`crate::files::split_env_files`]). Numeric keys are
    /// silently skipped rather than rejected: if a numeric key paired with a
    /// non-network value were ever set, `set`/`inject` would already have
    /// failed on it, so by the time `unset` runs there is nothing to clean
    /// up for it.
    pub fn keys_from_rules(
        rules: &[(PathBuf, LadeRule)],
        saved_user: &Option<String>,
    ) -> HashMap<Output, Vec<String>> {
        rules
            .iter()
            .map(|(_, rule)| {
                let keys = rule
                    .secrets
                    .iter()
                    .filter_map(
                        |(key, secret)| match resolve_entry(key, secret, saved_user) {
                            Some(ResolvedEntry::Secret { key, .. }) if is_valid_env_key(&key) => {
                                Some(key)
                            }
                            _ => None,
                        },
                    )
                    .collect::<Vec<_>>();
                (rule.config.as_ref().and_then(|c| c.file.clone()), keys)
            })
            .collect()
    }

    #[cfg(test)]
    pub fn collect_keys(&self, command: &str) -> HashMap<Output, Vec<String>> {
        Self::keys_from_rules(&self.collect(command), &None)
    }

    #[cfg(test)]
    pub async fn collect_keys_for_command(
        &self,
        command: &str,
    ) -> Result<HashMap<Output, Vec<String>>> {
        let saved_user = saved_user().await?;
        Ok(Self::keys_from_rules(&self.collect(command), &saved_user))
    }

    /// All disclaimers from already-collected `rules`, in order, deduplicated
    /// so the same text from several matching rules is shown only once.
    pub fn disclaimers_from_rules(rules: &[(PathBuf, LadeRule)]) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        rules
            .iter()
            .filter_map(|(_, rule)| rule.config.as_ref().and_then(|c| c.disclaimer.clone()))
            .filter(|d| seen.insert(d.clone()))
            .collect()
    }

    /// All disclaimers from rules matching `command`, in rule order, deduplicated
    /// so the same text from several matching rules is shown only once.
    #[cfg(test)]
    pub fn collect_disclaimers(&self, command: &str) -> Vec<String> {
        Self::disclaimers_from_rules(&self.collect(command))
    }

    pub fn all_secret_sources(&self, saved_user: &Option<String>) -> Vec<String> {
        self.rules
            .iter()
            .filter_map(|(_, rule)| rule_sources(rule, saved_user).ok())
            .flat_map(|sources| sources.into_values())
            .collect()
    }

    pub fn all_network_sources(&self, saved_user: &Option<String>) -> Vec<String> {
        self.rules
            .iter()
            .flat_map(|(_, rule)| {
                rule.secrets.iter().filter_map(|(key, secret)| {
                    match resolve_entry(key, secret, saved_user) {
                        Some(ResolvedEntry::Network { uri, .. }) => Some(uri),
                        _ => None,
                    }
                })
            })
            .collect()
    }

    /// Network bindings for already-collected `rules`, conflict-checked by
    /// key across all of them.
    pub fn network_bindings_from_rules(
        rules: &[(PathBuf, LadeRule)],
        saved_user: &Option<String>,
    ) -> Result<Vec<NetworkBinding>> {
        let mut by_key = HashMap::<String, String>::new();
        for (_, rule) in rules {
            for (key, secret) in &rule.secrets {
                let Some(ResolvedEntry::Network { key, uri }) =
                    resolve_entry(key, secret, saved_user)
                else {
                    continue;
                };
                match by_key.get(&key) {
                    Some(existing) if existing != &uri => bail!(
                        "conflicting network binding for '{}': '{}' and '{}' match the same command",
                        key,
                        existing,
                        uri
                    ),
                    Some(_) => {}
                    None => {
                        by_key.insert(key, uri);
                    }
                }
            }
        }
        Ok(by_key
            .into_iter()
            .map(|(key, uri)| NetworkBinding { key, uri })
            .collect())
    }

    #[cfg(test)]
    pub fn collect_network_bindings(
        &self,
        command: &str,
        saved_user: &Option<String>,
    ) -> Result<Vec<NetworkBinding>> {
        Self::network_bindings_from_rules(&self.collect(command), saved_user)
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
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
