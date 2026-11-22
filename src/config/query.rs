use super::resolve::{ResolvedEntry, body_entries, cancelled_name, resolve_entry, rule_sources};
use super::{Audience, Config, LadeRule, NetworkBinding, RuleTtl};

impl Config {
    pub fn all_secret_sources(&self, saved_user: &Option<String>) -> Vec<String> {
        self.rules
            .iter()
            .filter_map(|(_, rule)| rule_sources(rule, saved_user).ok())
            .flat_map(|sources| sources.into_values())
            .collect()
    }

    pub fn sources_for_command(
        &self,
        command: &str,
        saved_user: &Option<String>,
        audience: Audience,
    ) -> anyhow::Result<Vec<String>> {
        let mut by_key = indexmap::IndexMap::<String, String>::new();
        for (_, rule) in self.collect_for(command, audience)? {
            for entry in body_entries(&rule, saved_user) {
                // § sn2x1h4v9.b@sg3ng
                match entry {
                    ResolvedEntry::Secret { key, value } => {
                        by_key.insert(key, value);
                    }
                    ResolvedEntry::Tunnel { key, uri } => {
                        by_key.insert(key, uri);
                    }
                    ResolvedEntry::Unset { key } => {
                        let name = cancelled_name(&key);
                        by_key.shift_remove(name);
                        by_key.shift_remove(&format!(".{name}"));
                    }
                    ResolvedEntry::Pin { key, .. } | ResolvedEntry::Package { key, .. } => {
                        by_key.shift_remove(&key);
                    }
                    _ => {}
                }
            }
        }
        Ok(by_key.into_values().collect())
    }

    pub fn command_package_uri(
        &self,
        command: &str,
        saved_user: &Option<String>,
        audience: Audience,
    ) -> anyhow::Result<Option<String>> {
        Ok(Self::command_package(
            &self.collect_for_with_pattern(command, audience)?,
            saved_user,
        ))
    }

    /// The setup package a rule other than `.` binds for this command, which no
    /// door may run it with. A `.` body overlays it like any later body.
    pub(crate) fn command_package(
        patterned: &[(std::path::PathBuf, String, LadeRule)],
        saved_user: &Option<String>,
    ) -> Option<String> {
        let mut by_key = indexmap::IndexMap::<String, String>::new();
        for (_, pattern, rule) in patterned {
            // § vwj3wvrzs.a@b99js
            let dot = pattern == ".";
            for entry in body_entries(rule, saved_user) {
                // § sn2x1h4v9.b@7zk7t
                match entry {
                    ResolvedEntry::Package { key, uri } if !dot => {
                        by_key.insert(key, uri);
                    }
                    ResolvedEntry::Unset { key } => {
                        by_key.shift_remove(cancelled_name(&key));
                    }
                    ResolvedEntry::Package { key, .. }
                    | ResolvedEntry::Secret { key, .. }
                    | ResolvedEntry::Pin { key, .. }
                    | ResolvedEntry::Tunnel { key, .. } => {
                        by_key.shift_remove(&key);
                    }
                    _ => {}
                }
            }
        }
        by_key.into_values().next()
    }

    pub fn sources_in_dir(
        &self,
        dir: &std::path::Path,
        saved_user: &Option<String>,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for (path, rule) in &self.rules {
            if path != dir {
                continue;
            }
            if let Ok(sources) = rule_sources(rule, saved_user) {
                out.extend(sources.into_values());
            }
            for (key, secret) in &rule.secrets {
                match resolve_entry(key, secret, saved_user) {
                    Some(ResolvedEntry::Tunnel { uri, .. })
                    | Some(ResolvedEntry::Package { uri, .. }) => out.push(uri),
                    _ => {}
                }
            }
        }
        out
    }

    pub fn package_uris(&self, saved_user: &Option<String>) -> Vec<(String, String)> {
        let mut by_key = indexmap::IndexMap::<String, String>::new();
        for (_, rule) in &self.rules {
            for entry in body_entries(rule, saved_user) {
                // § sn2x1h4v9.b@dfdzf
                match entry {
                    ResolvedEntry::Package { key, uri } => {
                        by_key.insert(key, uri);
                    }
                    ResolvedEntry::Unset { key } => {
                        by_key.shift_remove(cancelled_name(&key));
                    }
                    ResolvedEntry::Secret { key, .. }
                    | ResolvedEntry::Pin { key, .. }
                    | ResolvedEntry::Tunnel { key, .. } => {
                        by_key.shift_remove(&key);
                    }
                    _ => {}
                }
            }
        }
        by_key.into_iter().collect()
    }

    pub fn all_network_sources(&self, saved_user: &Option<String>) -> Vec<String> {
        self.rules
            .iter()
            .flat_map(|(_, rule)| {
                rule.secrets.iter().filter_map(|(key, secret)| {
                    match resolve_entry(key, secret, saved_user) {
                        Some(ResolvedEntry::Tunnel { uri, .. }) => Some(uri),
                        _ => None,
                    }
                })
            })
            .collect()
    }

    pub fn network_bindings_from_rules(
        rules: &[(std::path::PathBuf, LadeRule)],
        saved_user: &Option<String>,
    ) -> Vec<NetworkBinding> {
        let mut by_key = std::collections::HashMap::<String, (String, Option<RuleTtl>)>::new();
        for (_, rule) in rules {
            for entry in body_entries(rule, saved_user) {
                // § sn2x1h4v9.b@nstx3
                match entry {
                    ResolvedEntry::Unset { key } => {
                        by_key.remove(cancelled_name(&key));
                    }
                    ResolvedEntry::Secret { key, .. }
                    | ResolvedEntry::Pin { key, .. }
                    | ResolvedEntry::Package { key, .. }
                        if !key.starts_with('.') =>
                    {
                        by_key.remove(&key);
                    }
                    ResolvedEntry::Tunnel { key, uri } if !key.starts_with('.') => {
                        by_key.insert(key, (uri, rule.ttl().cloned()));
                    }
                    _ => {}
                }
            }
        }
        by_key
            .into_iter()
            .map(|(key, (uri, ttl))| NetworkBinding { key, uri, ttl })
            .collect()
    }

    pub(crate) fn needs_wrap<'a>(
        work: &super::PreEventWork,
        command: &str,
        rules: impl IntoIterator<Item = &'a LadeRule>,
        saved_user: &Option<String>,
    ) -> bool {
        // § hk9h1sqyt.a@ty84h
        work.needs_inject() || Self::pin_wraps(rules, command, saved_user)
    }

    pub(crate) fn pins(&self, saved_user: &Option<String>) -> Vec<(String, String)> {
        self.pins_from_rules(self.rules.iter().map(|(_, rule)| rule), saved_user)
    }

    pub(crate) fn pins_for_command(
        &self,
        command: &str,
        saved_user: &Option<String>,
        audience: Audience,
    ) -> anyhow::Result<Vec<(String, String)>> {
        Ok(self.pins_from_rules(
            self.collect_for(command, audience)?
                .iter()
                .map(|(_, rule)| rule),
            saved_user,
        ))
    }

    pub(crate) fn pins_in_dir(
        &self,
        dir: &std::path::Path,
        saved_user: &Option<String>,
    ) -> Vec<(String, String)> {
        self.pins_from_rules(
            self.rules
                .iter()
                .filter(|(path, _)| path == dir)
                .map(|(_, rule)| rule),
            saved_user,
        )
    }

    fn pins_from_rules<'a>(
        &self,
        rules: impl IntoIterator<Item = &'a LadeRule>,
        saved_user: &Option<String>,
    ) -> Vec<(String, String)> {
        let mut by_key = indexmap::IndexMap::<String, String>::new();
        for rule in rules {
            for entry in body_entries(rule, saved_user) {
                // § sn2x1h4v9.b@wa5cx
                match entry {
                    ResolvedEntry::Pin { key, value } => {
                        by_key.insert(key, value);
                    }
                    ResolvedEntry::Unset { key } => {
                        by_key.shift_remove(cancelled_name(&key));
                    }
                    ResolvedEntry::Secret { key, .. }
                    | ResolvedEntry::Tunnel { key, .. }
                    | ResolvedEntry::Package { key, .. } => {
                        by_key.shift_remove(&key);
                    }
                    _ => {}
                }
            }
        }
        by_key.into_iter().collect()
    }

    fn pin_wraps<'a>(
        rules: impl IntoIterator<Item = &'a LadeRule>,
        command: &str,
        saved_user: &Option<String>,
    ) -> bool {
        let argv0 = crate::mise::argv0(command);
        let mut by_key = indexmap::IndexMap::<String, bool>::new();
        for rule in rules {
            for entry in body_entries(rule, saved_user) {
                // § one aq3qybz6p.b@rxkah, aq3qybz6p.d@9ktvp
                // A malformed shell pin still goes to the wrap, which parses
                // every pin first and fails the line.
                match entry {
                    ResolvedEntry::Pin { key, value }
                        if crate::mise::is_user_shell(&key)
                            && key != argv0
                            && crate::mise::parse_spec(&value).is_ok() => {}
                    ResolvedEntry::Pin { key, .. } => {
                        by_key.insert(key, true);
                    }
                    ResolvedEntry::Secret { key, value }
                        if crate::mise::looks_like_bare_version(&value) =>
                    {
                        by_key.insert(key, false);
                    }
                    ResolvedEntry::Unset { key } => {
                        by_key.shift_remove(cancelled_name(&key));
                    }
                    ResolvedEntry::Secret { key, .. }
                    | ResolvedEntry::Tunnel { key, .. }
                    | ResolvedEntry::Package { key, .. } => {
                        by_key.shift_remove(&key);
                    }
                    _ => {}
                }
            }
        }
        by_key.contains_key(argv0) || by_key.values().any(|is_pin| *is_pin)
    }

    pub(crate) fn bare_version_for(
        &self,
        command: &str,
        argv0: &str,
        saved_user: &Option<String>,
        audience: Audience,
    ) -> anyhow::Result<Option<String>> {
        let mut found = None;
        for (_, rule) in self.collect_for(command, audience)? {
            let Some(secret) = rule.secrets.get(argv0) else {
                continue;
            };
            // § sn2x1h4v9.b@136tm
            match resolve_entry(argv0, secret, saved_user) {
                Some(ResolvedEntry::Secret { value, .. })
                    if crate::mise::looks_like_bare_version(&value) =>
                {
                    found = Some(value);
                }
                Some(ResolvedEntry::Unset { .. })
                | Some(ResolvedEntry::Pin { .. })
                | Some(ResolvedEntry::Tunnel { .. })
                | Some(ResolvedEntry::Package { .. })
                | Some(ResolvedEntry::Secret { .. }) => {
                    found = None;
                }
                _ => {}
            }
        }
        Ok(found)
    }

    #[cfg(test)]
    pub fn collect_network_bindings(
        &self,
        command: &str,
        saved_user: &Option<String>,
    ) -> Vec<NetworkBinding> {
        Self::network_bindings_from_rules(&self.collect(command).unwrap(), saved_user)
    }
}
