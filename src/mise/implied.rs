//! Implied pins lock a product CLI that a URI needs, without writing it in
//! yaml.
//!
//! `op://vault/item/field` with no `op:` pin still locks `op` on
//! `lade setup`. `lade update` moves that lock to the latest bin
//! in the product range. Inject uses that store bin when it is present.
//! A miss refuses. Homebrew or another PATH binary is not used.
//! Same for `vault://`, `awssm://`, `apm://`, `skills://`, and the
//! other rows in this table. `ssh://` stays the OpenSSH binary on
//! PATH: there is no implied mise pin.

use std::sync::OnceLock;

use lade_sdk::compat::CLI_SPECS;

use crate::config::Config;

use super::spec::{self, Spec};

/// A scheme that implies a locked product CLI. ssh stays the OS binary.
pub struct Implied {
    pub scheme: &'static str,
    pub key: &'static str,
    pub uri: String,
}

fn range_uri(mise: &str, min: &str, max: Option<&str>) -> String {
    match max {
        Some(max) => format!("mise://{mise}@>={min},<{max}"),
        None => format!("mise://{mise}@>={min}"),
    }
}

fn table() -> &'static [Implied] {
    static TABLE: OnceLock<Vec<Implied>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut out = Vec::new();
        for spec in CLI_SPECS {
            let Some(mise) = spec.mise else {
                continue;
            };
            out.push(Implied {
                scheme: spec.scheme,
                key: spec.bin,
                uri: range_uri(mise, spec.min_version, spec.max_version),
            });
        }
        out
    })
}

pub fn by_scheme(scheme: &str) -> Option<&'static Implied> {
    table().iter().find(|row| row.scheme == scheme)
}

pub fn by_key(key: &str) -> Option<&'static Implied> {
    table().iter().find(|row| row.key == key)
}

pub fn pins_for(sources: &[String], existing: &[(String, Spec)]) -> Vec<(String, Spec)> {
    let mut out = Vec::new();
    for source in sources {
        let Some(scheme) = source.split_once("://").map(|(scheme, _)| scheme) else {
            continue;
        };
        let Some(row) = by_scheme(scheme) else {
            continue;
        };
        if existing
            .iter()
            .chain(out.iter())
            .any(|(key, spec)| key == row.key || spec.short_name() == row.key)
        {
            continue;
        }
        match spec::parse(&row.uri) {
            Ok(parsed) => out.push((row.key.to_string(), parsed)),
            Err(_) => continue,
        }
    }
    out
}

pub fn any_from_sources(sources: &[String]) -> bool {
    sources.iter().any(|source| {
        source
            .split_once("://")
            .is_some_and(|(scheme, _)| by_scheme(scheme).is_some())
    })
}

pub fn repo_needs_mise(config: &Config, saved: &Option<String>) -> bool {
    if !config.pins(saved).is_empty() {
        return true;
    }
    if !config.package_uris(saved).is_empty() {
        return true;
    }
    let mut sources = config.all_secret_sources(saved);
    sources.extend(config.all_network_sources(saved));
    any_from_sources(&sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_key_finds_aws() {
        assert_eq!(by_key("aws").map(|row| row.scheme), Some("awssm"));
        assert_eq!(by_scheme("awssm").map(|row| row.key), Some("aws"));
    }

    #[test]
    fn op_implies_lock() {
        let pins = pins_for(&["op://v/i/f".to_string()], &[]);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].0, "op");
        assert_eq!(pins[0].1.prefix, "aqua");
        assert_eq!(pins[0].1.package, "1password/op");
        assert!(pins[0].1.is_range());
        assert_eq!(pins[0].1.version, ">=2.18.0");
    }

    #[test]
    fn yaml_pin_wins() {
        let existing = vec![(
            "op".to_string(),
            spec::parse("mise://aqua/1password/op@2.31.0").unwrap(),
        )];
        let pins = pins_for(&["op://v/i/f".to_string()], &existing);
        assert!(pins.is_empty());
    }

    #[test]
    fn ssh_and_file_do_not_imply() {
        let pins = pins_for(
            &[
                "ssh://host".to_string(),
                "file:///tmp/x?query=.a".to_string(),
                "age://key".to_string(),
                "raw://x".to_string(),
            ],
            &[],
        );
        assert!(pins.is_empty());
    }

    #[test]
    fn table_uris_parse() {
        for row in table() {
            spec::parse(&row.uri).unwrap_or_else(|e| panic!("{}: {e}", row.uri));
            assert!(
                spec::parse(&row.uri).unwrap().is_range(),
                "{} should be a range",
                row.uri
            );
        }
    }

    #[test]
    fn passbolt_implies_lock() {
        let pins = pins_for(&["passbolt://host/id/password".to_string()], &[]);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].0, "passbolt");
        assert_eq!(pins[0].1.prefix, "github");
        assert_eq!(pins[0].1.package, "passbolt/go-passbolt-cli");
        assert_eq!(pins[0].1.version, ">=0.5.0");
    }

    #[test]
    fn apm_implies_lock() {
        let pins = pins_for(&["apm://github/destructure-command-hook".to_string()], &[]);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].0, "apm");
        assert_eq!(pins[0].1.prefix, "github");
        assert_eq!(pins[0].1.package, "microsoft/apm");
        assert_eq!(pins[0].1.version, ">=0.23.1");
    }

    #[test]
    fn kubectl_implies_lock() {
        let pins = pins_for(
            &["kubectl://127.0.0.1:6443/ctx/ns/service/db/5432".to_string()],
            &[],
        );
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].0, "kubectl");
        assert_eq!(pins[0].1.prefix, "aqua");
        assert_eq!(pins[0].1.package, "kubernetes/kubectl");
        assert_eq!(pins[0].1.version, ">=1.27.0");
    }

    #[test]
    fn vault_implies_lock() {
        let pins = pins_for(&["vault://127.0.0.1:8200/secret/k/f".to_string()], &[]);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].0, "vault");
        assert_eq!(pins[0].1.prefix, "aqua");
        assert_eq!(pins[0].1.package, "hashicorp/vault");
        assert_eq!(pins[0].1.version, ">=1.15.0");
    }
}
