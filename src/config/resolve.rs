use anyhow::{Result, bail};
use std::collections::HashMap;

use crate::family::{Family, is_package_uri};

use super::secret::resolve_lade_secret;
use super::{LadeRule, LadeSecret};

/// A single rule entry, resolved for a user and classified as either a plain
/// secret/file value, a mise pin, or a tunnel binding
/// (kubectl://, kubefwd://, tsh://). Centralizing this classification keeps
/// the scheme/numeric-key rules consistent across hydration, `unset`, and
/// tunnel binding collection, instead of each call site re-deriving them
/// slightly differently.
pub(super) enum ResolvedEntry {
    Secret {
        key: String,
        value: String,
    },
    Pin {
        key: String,
        value: String,
    },
    /// Family bin. `apm://` and `skills://`, not a `mise://` pin.
    Package {
        key: String,
        uri: String,
    },
    Tunnel {
        key: String,
        uri: String,
    },
    /// A numeric key (port number) resolved to a non-tunnel value. Only
    /// `rule_sources`/`network_bindings_from_rules` treat this as an error;
    /// `keys_from_rules` (used for `unset`) just skips it, since by the time
    /// `unset` runs, `set`/`inject` would already have failed on it.
    InvalidNumericSecret {
        key: String,
    },
    Unset {
        key: String,
    },
}

pub(super) fn resolve_entry(
    key: &str,
    secret: &LadeSecret,
    saved_user: &Option<String>,
) -> Option<ResolvedEntry> {
    if matches!(secret, LadeSecret::Unset) {
        return Some(ResolvedEntry::Unset {
            key: key.to_string(),
        });
    }
    let value = resolve_lade_secret(secret, saved_user)?;
    match Family::of_uri(&value) {
        Family::Bin if is_package_uri(&value) => {
            return Some(ResolvedEntry::Package {
                key: key.to_string(),
                uri: value,
            });
        }
        Family::Bin => {
            return Some(ResolvedEntry::Pin {
                key: key.to_string(),
                value,
            });
        }
        Family::Tunnel => {
            return Some(ResolvedEntry::Tunnel {
                key: key.to_string(),
                uri: value,
            });
        }
        Family::Secret => {}
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

pub(super) fn split_scheme(value: &str) -> Option<&str> {
    value.split_once("://").map(|(scheme, _)| scheme)
}

pub(super) fn binding_name(key: &str) -> Result<(String, bool)> {
    if key == "." {
        bail!("'.' is reserved for rule configuration");
    }
    if let Some(name) = key.strip_prefix('.') {
        if name.is_empty() || !super::is_valid_env_key(name) {
            bail!("private binding '{key}' must be .NAME");
        }
        return Ok((name.to_string(), true));
    }
    Ok((key.to_string(), false))
}

/// Secret values only (no tunnel bindings) for a single rule, keyed by
/// name. Used for hydration, so it applies to both env and file-routed
/// outputs alike — unlike [`Config::keys_from_rules`], it does not require
/// keys to look like valid env var names (a file-routed secret can use any
/// key as its JSON/YAML field name).
pub(super) fn rule_sources(
    rule: &LadeRule,
    saved_user: &Option<String>,
) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for (key, secret) in &rule.secrets {
        match resolve_entry(key, secret, saved_user) {
            Some(ResolvedEntry::Secret { key, value }) => {
                out.insert(key, value);
            }
            Some(ResolvedEntry::Tunnel { .. })
            | Some(ResolvedEntry::Pin { .. })
            | Some(ResolvedEntry::Package { .. })
            | Some(ResolvedEntry::Unset { .. })
            | None => {}
            Some(ResolvedEntry::InvalidNumericSecret { key }) => bail!(
                "numeric key '{}' must use a tunnel URI (kubectl://, kubefwd://, tsh://)",
                key
            ),
        }
    }
    Ok(out)
}
