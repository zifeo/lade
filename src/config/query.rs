use super::resolve::{ResolvedEntry, resolve_entry, rule_sources};
use super::{Config, LadeRule, NetworkBinding};

impl Config {
    pub fn all_secret_sources(&self, saved_user: &Option<String>) -> Vec<String> {
        self.rules
            .iter()
            .filter_map(|(_, rule)| rule_sources(rule, saved_user).ok())
            .flat_map(|sources| sources.into_values())
            .collect()
    }

    pub fn sources_for_command(&self, command: &str, saved_user: &Option<String>) -> Vec<String> {
        let mut out = Vec::new();
        for (_, rule) in self.collect(command) {
            if let Ok(sources) = rule_sources(&rule, saved_user) {
                out.extend(sources.into_values());
            }
            for (key, secret) in &rule.secrets {
                if let Some(ResolvedEntry::Tunnel { uri, .. }) =
                    resolve_entry(key, secret, saved_user)
                {
                    out.push(uri);
                }
            }
        }
        out
    }

    pub fn command_package_uri(
        &self,
        command: &str,
        saved_user: &Option<String>,
    ) -> Option<String> {
        for i in self.compiled.matching_indices(command) {
            if self.patterns[i] == "." {
                continue;
            }
            for (key, secret) in &self.rules[i].1.secrets {
                if let Some(ResolvedEntry::Package { uri, .. }) =
                    resolve_entry(key, secret, saved_user)
                {
                    return Some(uri);
                }
            }
        }
        None
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
            for (key, secret) in &rule.secrets {
                match resolve_entry(key, secret, saved_user) {
                    Some(ResolvedEntry::Package { key, uri }) => {
                        by_key.insert(key, uri);
                    }
                    Some(ResolvedEntry::Unset { key })
                    | Some(ResolvedEntry::Secret { key, .. })
                    | Some(ResolvedEntry::Pin { key, .. })
                    | Some(ResolvedEntry::Tunnel { key, .. }) => {
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
        let mut by_key = std::collections::HashMap::<String, String>::new();
        for (_, rule) in rules {
            for (key, secret) in &rule.secrets {
                match resolve_entry(key, secret, saved_user) {
                    Some(ResolvedEntry::Unset { key })
                    | Some(ResolvedEntry::Secret { key, .. })
                    | Some(ResolvedEntry::Pin { key, .. })
                    | Some(ResolvedEntry::Package { key, .. })
                        if !key.starts_with('.') =>
                    {
                        by_key.remove(&key);
                    }
                    Some(ResolvedEntry::Tunnel { key, uri }) if !key.starts_with('.') => {
                        by_key.insert(key, uri);
                    }
                    _ => {}
                }
            }
        }
        by_key
            .into_iter()
            .map(|(key, uri)| NetworkBinding { key, uri })
            .collect()
    }

    pub(crate) fn needs_wrap<'a>(
        work: &super::PreEventWork,
        command: &str,
        rules: impl IntoIterator<Item = &'a LadeRule>,
        saved_user: &Option<String>,
    ) -> bool {
        work.needs_inject() || Self::pin_wraps(rules, command, saved_user)
    }

    pub(crate) fn pins(&self, saved_user: &Option<String>) -> Vec<(String, String)> {
        self.pins_from_rules(self.rules.iter().map(|(_, rule)| rule), saved_user)
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
            for (key, secret) in &rule.secrets {
                match resolve_entry(key, secret, saved_user) {
                    Some(ResolvedEntry::Pin { key, value }) => {
                        by_key.insert(key, value);
                    }
                    Some(ResolvedEntry::Unset { key })
                    | Some(ResolvedEntry::Secret { key, .. })
                    | Some(ResolvedEntry::Tunnel { key, .. })
                    | Some(ResolvedEntry::Package { key, .. }) => {
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
        let mut has_pin = false;
        for rule in rules {
            for (key, secret) in &rule.secrets {
                match resolve_entry(key, secret, saved_user) {
                    Some(ResolvedEntry::Pin { key, .. }) => {
                        has_pin = true;
                        if key == argv0 {
                            return true;
                        }
                    }
                    Some(ResolvedEntry::Secret { key, value })
                        if key == argv0 && crate::mise::looks_like_bare_version(&value) =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
        }
        has_pin && crate::mise::is_mise_argv0(argv0)
    }

    pub(crate) fn bare_version_for(
        &self,
        command: &str,
        argv0: &str,
        saved_user: &Option<String>,
    ) -> Option<String> {
        for (_, rule) in self.collect(command) {
            let Some(secret) = rule.secrets.get(argv0) else {
                continue;
            };
            if let Some(ResolvedEntry::Secret { value, .. }) =
                resolve_entry(argv0, secret, saved_user)
                && crate::mise::looks_like_bare_version(&value)
            {
                return Some(value);
            }
        }
        None
    }

    #[cfg(test)]
    pub fn collect_network_bindings(
        &self,
        command: &str,
        saved_user: &Option<String>,
    ) -> Vec<NetworkBinding> {
        Self::network_bindings_from_rules(&self.collect(command), saved_user)
    }
}
