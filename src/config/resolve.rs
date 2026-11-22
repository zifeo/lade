use anyhow::{Result, bail};
use std::collections::HashMap;

use crate::family::{Family, is_package_uri};

use super::secret::{cancels, resolve_lade_secret};
use super::{LadeRule, LadeSecret};

/// A single rule entry, resolved for a user and classified as either a plain
/// secret/file value, a mise pin, or a tunnel binding
/// (kubectl://, kubefwd://, tsh://). Centralizing this classification keeps
/// the scheme/numeric-key rules consistent across hydration, `unset`, and
/// tunnel binding collection, instead of each call site re-deriving them
/// slightly differently.
pub(crate) enum ResolvedEntry {
    Secret {
        key: String,
        value: String,
    },
    Pin {
        key: String,
        value: String,
    },
    /// Family package. `apm://` and `skills://`, not a `mise://` pin.
    Package {
        key: String,
        uri: String,
    },
    Tunnel {
        key: String,
        uri: String,
    },
    /// A numeric key (port number) resolved to a non-tunnel value. Only
    /// `rule_sources`/`network_bindings_from_rules` treat this as an error.
    InvalidNumericSecret {
        key: String,
    },
    Unset {
        key: String,
    },
}

pub(crate) fn resolve_entry(
    key: &str,
    secret: &LadeSecret,
    saved_user: &Option<String>,
) -> Option<ResolvedEntry> {
    if cancels(secret, saved_user) {
        return Some(ResolvedEntry::Unset {
            key: key.to_string(),
        });
    }
    // § one dcrxp986s.a@ppapf
    let value = resolve_lade_secret(secret, saved_user)?;
    match Family::of_uri(&value) {
        Family::Package if is_package_uri(&value) => {
            return Some(ResolvedEntry::Package {
                key: key.to_string(),
                uri: value,
            });
        }
        Family::Package => {
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

impl ResolvedEntry {
    fn key(&self) -> &str {
        match self {
            ResolvedEntry::Secret { key, .. }
            | ResolvedEntry::Pin { key, .. }
            | ResolvedEntry::Package { key, .. }
            | ResolvedEntry::Tunnel { key, .. }
            | ResolvedEntry::InvalidNumericSecret { key }
            | ResolvedEntry::Unset { key } => key,
        }
    }
}

/// The name a `~` on `key` cancels: `NAME` and `.NAME` cancel the same
/// binding, whatever its family.
pub(crate) fn cancelled_name(key: &str) -> &str {
    // § sn2x1h4v9.d@bft7e
    key.strip_prefix('.').unwrap_or(key)
}

/// A rule body's entries in the order they apply: its cancellations, then
/// its values, each by key. A `~` removes what an earlier body set, never a
/// value of its own body, and the body's map has no order of its own.
pub(crate) fn body_entries(rule: &LadeRule, saved_user: &Option<String>) -> Vec<ResolvedEntry> {
    // § sn2x1h4v9.d@17c7g
    let mut entries = rule
        .secrets
        .iter()
        .filter_map(|(key, secret)| resolve_entry(key, secret, saved_user))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        let value = |entry: &ResolvedEntry| !matches!(entry, ResolvedEntry::Unset { .. });
        (value(a), a.key()).cmp(&(value(b), b.key()))
    });
    // § end sn2x1h4v9.d
    entries
}

pub(super) fn split_scheme(value: &str) -> Option<&str> {
    value.split_once("://").map(|(scheme, _)| scheme)
}

/// A binding exported to the env lands in shell code the pre-exec hook
/// evals. One written to a file can use any field name.
pub(super) fn check_env_name(name: &str, to_file: bool) -> Result<()> {
    // § ps6s9115k@9ajwq
    if !to_file && !super::is_valid_env_key(name) {
        bail!(
            "binding '{name}' is not a valid env var name: use letters, digits, and _, not starting with a digit"
        );
    }
    Ok(())
}

/// Name of a binding that removes or replaces an earlier one. A `.NAME` only
/// builds another value, which a tunnel or a package cannot do.
pub(super) fn replacing_name(key: &str, holds_secret: bool) -> Result<String> {
    let (name, private) = binding_name(key)?;
    if private && !holds_secret {
        // § dyat01nyj.b@pp6xs
        bail!("private binding '{key}' must hold a secret, not a tunnel or a package");
    }
    Ok(name)
}

pub(crate) fn binding_name(key: &str) -> Result<(String, bool)> {
    if key == "." {
        bail!("'.' is reserved for rule configuration");
    }
    if let Some(name) = key.strip_prefix('.') {
        // § dyat01nyj.b@zrcd6
        if name.is_empty() || !super::is_valid_env_key(name) {
            bail!("private binding '{key}' must be .NAME");
        }
        return Ok((name.to_string(), true));
    }
    Ok((key.to_string(), false))
}

/// Secret values only (no tunnel bindings) for a single rule, keyed by
/// name. Used for hydration, so it applies to both env and file-routed
/// outputs alike: it does not require keys to look like valid env var names
/// (a file-routed secret can use any key as its JSON/YAML field name).
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
