use anyhow::Result;
use std::path::PathBuf;

use super::plan::{
    ResolvedEntry, binding_name, op_sa_from_rules, resolve_entry, secrets_from_rules,
};
use super::{Audience, Config, LadeRule, PreEventWork, RuleWhen};
use crate::event::match_tree_from;

fn rule_applies_to(rule: &LadeRule, audience: Audience) -> bool {
    match rule
        .config
        .as_ref()
        .map(|config| config.when)
        .unwrap_or_default()
    {
        RuleWhen::Always => true,
        RuleWhen::Human => audience == Audience::Human,
        RuleWhen::Agent => audience == Audience::Agent,
    }
}

impl Config {
    /// Rules matching `command`, in overlay order: parent `lade.yml` then
    /// child, and top-to-bottom within a file. Later entries replace the same
    /// key. Callers on the hot path should call this once per invocation and
    /// reuse the result, rather than letting each downstream step
    /// (disclaimers, network bindings, secret sources, hydration) re-match
    /// independently.
    /// Last explicit `log` on matching rules wins. Absent `log` is not a vote.
    pub(crate) fn log_enabled(rules: &[(PathBuf, LadeRule)]) -> bool {
        let mut enabled = false;
        for (_, rule) in rules {
            if let Some(flag) = rule.config.as_ref().and_then(|config| config.log) {
                enabled = flag;
            }
        }
        enabled
    }

    /// Last explicit `log` on all loaded rules wins. Absent `log` is not a vote.
    /// Used for pre-event `log` and walk-level diary policy.
    pub(crate) fn log_on_walk(&self) -> bool {
        let mut enabled = false;
        for (_, rule) in &self.rules {
            if let Some(flag) = rule.config.as_ref().and_then(|config| config.log) {
                enabled = flag;
            }
        }
        enabled
    }

    /// Build pre-event work from already-matched rules. No hydration.
    /// [`PreEventWork::log`] is last explicit `log` on these matches. No-match
    /// `seen` uses [`Self::log_on_walk`].
    pub(crate) fn pre_event_work(
        rules: &[(PathBuf, String, LadeRule)],
        saved_user: &Option<String>,
    ) -> Result<PreEventWork> {
        let plain = rules
            .iter()
            .map(|(path, _, rule)| (path.clone(), rule.clone()))
            .collect::<Vec<_>>();
        Ok(PreEventWork {
            disclaimers: Self::disclaimers_from_rules(&plain),
            secrets: secrets_from_rules(&plain, saved_user)?,
            network: Self::network_bindings_from_rules(&plain, saved_user),
            matches: match_tree_from(rules, saved_user),
            op_sa: op_sa_from_rules(&plain, saved_user),
            log: Self::log_enabled(&plain),
            progress: Self::secret_sources_from_rules(&plain, saved_user)?,
        })
    }

    pub(crate) fn collect(&self, command: &str) -> Vec<(PathBuf, LadeRule)> {
        self.compiled
            .matching_indices(command)
            .into_iter()
            .map(|i| self.rules[i].clone())
            .collect()
    }

    pub(crate) fn collect_for(
        &self,
        command: &str,
        audience: Audience,
    ) -> Vec<(PathBuf, LadeRule)> {
        self.collect(command)
            .into_iter()
            .filter(|(_, rule)| rule_applies_to(rule, audience))
            .collect()
    }

    pub(crate) fn collect_for_with_pattern(
        &self,
        command: &str,
        audience: Audience,
    ) -> Vec<(PathBuf, String, LadeRule)> {
        self.compiled
            .matching_indices(command)
            .into_iter()
            .filter(|&i| rule_applies_to(&self.rules[i].1, audience))
            .map(|i| {
                (
                    self.rules[i].0.clone(),
                    self.patterns[i].clone(),
                    self.rules[i].1.clone(),
                )
            })
            .collect()
    }

    pub(crate) fn public_bindings(
        rule: &LadeRule,
        saved_user: &Option<String>,
    ) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (key, secret) in &rule.secrets {
            let Ok((name, private)) = binding_name(key) else {
                continue;
            };
            if private {
                continue;
            }
            match resolve_entry(&name, secret, saved_user) {
                Some(ResolvedEntry::Secret { key, value })
                | Some(ResolvedEntry::Network { key, uri: value }) => {
                    out.push((key, value));
                }
                _ => {}
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
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
}
