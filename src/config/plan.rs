use anyhow::{Result, bail};
use lade_sdk::{Dag, Template};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::resolve::{
    ResolvedEntry, binding_name, body_entries, cancelled_name, check_env_name, replacing_name,
};
use super::secret::resolve_lade_secret;
use super::{Config, LadeRule, SecretSources};
use crate::ticket::TicketSecret;

/// A public binding and a `.NAME` of one name cannot both apply, whatever
/// the family of the public one.
fn declare(held: &mut HashMap<String, bool>, name: &str, private: bool) -> Result<()> {
    // § dyat01nyj.a@6h9aq
    if held
        .insert(name.to_string(), private)
        .is_some_and(|was| was != private)
    {
        bail!("binding '{name}' is declared both public and private");
    }
    Ok(())
}

pub(super) fn secrets_from_rules(
    rules: &[(PathBuf, LadeRule)],
    saved_user: &Option<String>,
) -> Result<Vec<TicketSecret>> {
    let mut bindings = HashMap::<String, TicketSecret>::new();
    let mut held = HashMap::<String, bool>::new();
    for (cwd, rule) in rules {
        let output = rule.config.as_ref().and_then(|config| config.file.clone());
        // § zk7sjvann.b@aqvwk
        // § one zk7sjvann.a@c7kbv
        let rule_op_sa = rule
            .config
            .as_ref()
            .and_then(|config| config.onepassword_service_account.as_ref())
            .and_then(|secret| resolve_lade_secret(secret, saved_user));
        for entry in body_entries(rule, saved_user) {
            // § sn2x1h4v9.b@61t1n
            match entry {
                ResolvedEntry::Unset { key } => {
                    let name = replacing_name(&key, true)?;
                    held.remove(&name);
                    bindings.remove(&name);
                }
                ResolvedEntry::Tunnel { key, .. }
                | ResolvedEntry::Pin { key, .. }
                | ResolvedEntry::Package { key, .. } => {
                    let name = replacing_name(&key, false)?;
                    declare(&mut held, &name, false)?;
                    bindings.remove(&name);
                }
                ResolvedEntry::InvalidNumericSecret { key } => bail!(
                    "numeric key '{}' must use a tunnel URI (kubectl://, kubefwd://, tsh://)",
                    key
                ),
                ResolvedEntry::Secret { key, value } => {
                    // § ps6s9115k.a@wr312
                    let (name, private) = binding_name(&key)?;
                    check_env_name(&name, output.is_some())?;
                    let ticket = TicketSecret {
                        key: name.clone(),
                        source: value,
                        private,
                        output: output.as_ref().map(|path| cwd.join(path)),
                        cwd: cwd.clone(),
                        op_sa: rule_op_sa.clone(),
                    };
                    declare(&mut held, &name, private)?;
                    bindings.insert(name, ticket);
                }
            }
        }
    }
    let mut secrets = bindings.into_values().collect::<Vec<_>>();
    secrets.sort_by(|a, b| a.key.cmp(&b.key));
    // § v3rjmav3z.b@3662w
    Dag::new(
        secrets
            .iter()
            .map(|secret| (secret.key.clone(), Template::parse(&secret.source)))
            .collect(),
    )?;
    Ok(secrets)
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
        let mut not_secrets = HashSet::<String>::new();
        for (_, rule) in rules {
            let silent = rule.config.as_ref().is_some_and(|config| config.silent);
            for entry in body_entries(rule, saved_user) {
                // § sn2x1h4v9.c@css46
                match entry {
                    ResolvedEntry::Secret { key, value } => {
                        let replaced_other = not_secrets.remove(&key);
                        if replaced_other
                            || plan.sources.contains_key(&key)
                            || plan.cancelled.contains_key(&key)
                        {
                            plan.overridden.insert(key.clone());
                        }
                        plan.cancelled.remove(&key);
                        mark_silent(&mut plan.silent, &key, silent);
                        plan.sources.insert(key, value);
                    }
                    ResolvedEntry::Unset { key } => {
                        let name = cancelled_name(&key);
                        let held = [name.to_string(), format!(".{name}")]
                            .into_iter()
                            .filter(|form| {
                                plan.sources.contains_key(form) || not_secrets.contains(form)
                            })
                            .collect::<Vec<_>>();
                        let cancelled = if held.is_empty() { vec![key] } else { held };
                        for key in cancelled {
                            not_secrets.remove(&key);
                            plan.overridden.remove(&key);
                            let previous = plan.sources.remove(&key).unwrap_or_default();
                            mark_silent(&mut plan.silent, &key, silent);
                            plan.cancelled.insert(key, previous);
                        }
                    }
                    ResolvedEntry::Tunnel { key, .. }
                    | ResolvedEntry::Pin { key, .. }
                    | ResolvedEntry::Package { key, .. } => {
                        plan.overridden.remove(&key);
                        plan.cancelled.remove(&key);
                        plan.silent.remove(&key);
                        plan.sources.remove(&key);
                        not_secrets.insert(key);
                    }
                    ResolvedEntry::InvalidNumericSecret { key } => bail!(
                        "numeric key '{}' must use a tunnel URI (kubectl://, kubefwd://, tsh://)",
                        key
                    ),
                }
            }
        }
        Ok(plan)
    }

    #[cfg(test)]
    pub fn collect_secret_sources(&self, command: &str) -> Result<SecretSources> {
        Self::secret_sources_from_rules(&self.collect(command)?, &None)
    }
}
