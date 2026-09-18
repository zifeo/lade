use anyhow::{Result, bail};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use super::resolve::{ResolvedEntry, binding_name, resolve_entry};
use super::secret::resolve_lade_secret;
use super::{Config, LadeRule, Output, SecretSources};
use crate::ticket::TicketSecret;

pub(super) fn secrets_from_rules(
    rules: &[(PathBuf, LadeRule)],
    saved_user: &Option<String>,
) -> Result<Vec<TicketSecret>> {
    let mut bindings = HashMap::<String, TicketSecret>::new();
    for (cwd, rule) in rules {
        let output = rule.config.as_ref().and_then(|config| config.file.clone());
        for (key, secret) in &rule.secrets {
            match resolve_entry(key, secret, saved_user) {
                Some(ResolvedEntry::Unset { key })
                | Some(ResolvedEntry::Tunnel { key, .. })
                | Some(ResolvedEntry::Pin { key, .. })
                | Some(ResolvedEntry::Package { key, .. }) => {
                    let (name, _) = binding_name(&key)?;
                    bindings.remove(&name);
                }
                Some(ResolvedEntry::InvalidNumericSecret { key }) => bail!(
                    "numeric key '{}' must use a tunnel URI (kubectl://, kubefwd://, tsh://)",
                    key
                ),
                None => {}
                Some(ResolvedEntry::Secret { key, value }) => {
                    let (name, private) = binding_name(&key)?;
                    let ticket = TicketSecret {
                        key: name.clone(),
                        source: value,
                        private,
                        output: output.as_ref().map(|path| cwd.join(path)),
                        cwd: cwd.clone(),
                    };
                    if let Some(existing) = bindings.get(&name)
                        && existing.private != ticket.private
                    {
                        bail!("binding '{name}' is declared both public and private");
                    }
                    bindings.insert(name, ticket);
                }
            }
        }
    }
    let mut secrets = bindings.into_values().collect::<Vec<_>>();
    secrets.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(secrets)
}

pub(super) fn op_sa_from_rules(
    rules: &[(PathBuf, LadeRule)],
    saved_user: &Option<String>,
) -> Option<String> {
    let mut op_sa = None;
    for (_, rule) in rules {
        if let Some(secret) = rule
            .config
            .as_ref()
            .and_then(|config| config.onepassword_service_account.as_ref())
        {
            op_sa = resolve_lade_secret(secret, saved_user);
        }
    }
    op_sa
}

fn mark_silent(silent: &mut HashSet<String>, key: &str, is_silent: bool) {
    if is_silent {
        silent.insert(key.to_string());
    } else {
        silent.remove(key);
    }
}

impl Config {
    /// Secret sources only (no hydration) for already-collected `rules`.
    /// Used to display provider progress groups before hydration runs.
    pub fn secret_sources_from_rules(
        rules: &[(PathBuf, LadeRule)],
        saved_user: &Option<String>,
    ) -> Result<SecretSources> {
        let mut plan = SecretSources::default();
        for (_, rule) in rules {
            let silent = rule.config.as_ref().is_some_and(|config| config.silence);
            for (key, secret) in &rule.secrets {
                match resolve_entry(key, secret, saved_user) {
                    Some(ResolvedEntry::Secret { key, value }) => {
                        if plan.sources.contains_key(&key) || plan.cancelled.contains_key(&key) {
                            plan.overridden.insert(key.clone());
                        }
                        plan.cancelled.remove(&key);
                        mark_silent(&mut plan.silent, &key, silent);
                        plan.sources.insert(key, value);
                    }
                    Some(ResolvedEntry::Unset { key }) => {
                        plan.overridden.remove(&key);
                        let previous = plan.sources.remove(&key).unwrap_or_default();
                        mark_silent(&mut plan.silent, &key, silent);
                        plan.cancelled.insert(key, previous);
                    }
                    Some(ResolvedEntry::Tunnel { key, .. })
                    | Some(ResolvedEntry::Pin { key, .. })
                    | Some(ResolvedEntry::Package { key, .. }) => {
                        plan.overridden.remove(&key);
                        plan.cancelled.remove(&key);
                        plan.silent.remove(&key);
                        plan.sources.remove(&key);
                    }
                    Some(ResolvedEntry::InvalidNumericSecret { key }) => bail!(
                        "numeric key '{}' must use a tunnel URI (kubectl://, kubefwd://, tsh://)",
                        key
                    ),
                    None => {}
                }
            }
        }
        Ok(plan)
    }

    /// Env var names per [`Output`] for already-collected `rules`, used to
    /// remove temporary files on `unset`. Numeric keys are skipped: `set` /
    /// `inject` would already have failed on a numeric non-network value.
    pub fn keys_from_rules(
        rules: &[(PathBuf, LadeRule)],
        saved_user: &Option<String>,
    ) -> HashMap<Output, Vec<String>> {
        let mut by_output: HashMap<Output, BTreeSet<String>> = HashMap::new();
        for (_, rule) in rules {
            let output = rule.config.as_ref().and_then(|c| c.file.clone());
            let keys = by_output.entry(output).or_default();
            for (key, secret) in &rule.secrets {
                if key.starts_with('.') || !super::is_valid_env_key(key) {
                    continue;
                }
                match resolve_entry(key, secret, saved_user) {
                    Some(ResolvedEntry::Secret { key, .. }) => {
                        keys.insert(key);
                    }
                    Some(ResolvedEntry::Unset { key })
                    | Some(ResolvedEntry::Tunnel { key, .. })
                    | Some(ResolvedEntry::Pin { key, .. })
                    | Some(ResolvedEntry::Package { key, .. }) => {
                        keys.remove(&key);
                    }
                    _ => {}
                }
            }
        }
        by_output
            .into_iter()
            .filter(|(_, keys)| !keys.is_empty())
            .map(|(output, keys)| (output, keys.into_iter().collect()))
            .collect()
    }

    #[cfg(test)]
    pub fn collect_secret_sources(&self, command: &str) -> Result<SecretSources> {
        Self::secret_sources_from_rules(&self.collect(command), &None)
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
        let saved_user = super::saved_user().await?;
        Ok(Self::keys_from_rules(&self.collect(command), &saved_user))
    }
}
