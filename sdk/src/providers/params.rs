use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};

use super::env_lookup;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pairs: HashMap<String, String>,
}

impl Query {
    pub fn parse(raw: &str) -> Self {
        let pairs = url::form_urlencoded::parse(raw.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        Self { pairs }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs.get(key).map(String::as_str)
    }
}

pub fn split_scheme_path_query(value: &str, scheme: &str) -> Result<(String, Query)> {
    let prefix = format!("{scheme}://");
    let rest = value
        .strip_prefix(&prefix)
        .ok_or_else(|| anyhow!("Not a {scheme} scheme"))?;
    if rest.is_empty() || rest.starts_with('?') {
        bail!("{scheme}:// path cannot be empty");
    }
    let (path, raw_query) = match rest.split_once('?') {
        Some((path, query)) => (path, query),
        None => (rest, ""),
    };
    if path.is_empty() {
        bail!("{scheme}:// path cannot be empty");
    }
    Ok((path.to_string(), Query::parse(raw_query)))
}

pub fn require_named_env(
    extra_env: &HashMap<String, String>,
    var_name: &str,
    what: &str,
    docs: &str,
) -> Result<String> {
    env_lookup(extra_env, &[var_name])
        .ok_or_else(|| anyhow!("{what}: environment variable {var_name} is not set. See {docs}."))
}

pub fn age_plugin_bin(plugin: &str) -> String {
    format!("age-plugin-{plugin}")
}

pub fn require_age_plugin_on_path(
    extra_env: &HashMap<String, String>,
    plugin: &str,
    docs: &str,
) -> Result<()> {
    let bin = age_plugin_bin(plugin);
    if find_on_path(extra_env, &bin).is_some() {
        return Ok(());
    }
    bail!("{bin} not found on PATH. Install the plugin or add it to PATH. See {docs}.");
}

fn find_on_path(extra_env: &HashMap<String, String>, bin: &str) -> Option<std::path::PathBuf> {
    let path = extra_env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn parse_plugin_name(raw: &str) -> Result<String> {
    if raw.is_empty() {
        bail!("plugin name cannot be empty");
    }
    if let Some(name) = raw.strip_prefix("age-plugin-") {
        bail!("use plugin={name}, not the binary name {raw}");
    }
    let ok = raw.chars().enumerate().all(|(i, c)| match c {
        'a'..='z' | '0'..='9' => true,
        '_' | '-' | '.' | '+' => i > 0,
        _ => false,
    });
    if !ok {
        bail!("invalid plugin name {raw}");
    }
    Ok(raw.to_string())
}

pub fn apply_process_path(extra_env: &HashMap<String, String>) {
    let Some(extra) = extra_env.get("PATH") else {
        return;
    };
    let mut dirs: Vec<_> = std::env::split_paths(extra).collect();
    if let Ok(current) = std::env::var("PATH") {
        dirs.extend(std::env::split_paths(&current));
    }
    if let Ok(merged) = std::env::join_paths(dirs) {
        // The age crate resolves plugins via process PATH, not extra_env.
        unsafe { std::env::set_var("PATH", merged) };
    }
}

pub fn require_identity_names(query: &Query, plugin: Option<&str>, docs: &str) -> Result<()> {
    if plugin.is_none() {
        return Ok(());
    }
    if query.get("identity").is_some() || query.get("identity_file").is_some() {
        return Ok(());
    }
    bail!(
        "plugin {} requires identity=ENV or identity_file=ENV in the URI. See {docs}.",
        plugin.unwrap()
    );
}

pub fn load_named_identity(
    extra_env: &HashMap<String, String>,
    query: &Query,
    fallback_key: &[&str],
    fallback_file: &[&str],
    what: &str,
    docs: &str,
) -> Result<String> {
    if let Some(var) = query.get("identity") {
        return require_named_env(extra_env, var, what, docs);
    }
    if let Some(var) = query.get("identity_file") {
        let path = require_named_env(extra_env, var, what, docs)?;
        return std::fs::read_to_string(&path)
            .map_err(|e| anyhow!("{what}: identity file {path}: {e}. See {docs}."));
    }
    if let Some(key) = env_lookup(extra_env, fallback_key) {
        return Ok(key);
    }
    if let Some(path) = env_lookup(extra_env, fallback_file) {
        return std::fs::read_to_string(&path)
            .map_err(|e| anyhow!("{what}: identity file {path}: {e}. See {docs}."));
    }
    let names = fallback_key
        .iter()
        .chain(fallback_file.iter())
        .copied()
        .collect::<Vec<_>>()
        .join(" or ");
    bail!("{what}: set {names}, or identity=ENV / identity_file=ENV in the URI. See {docs}.");
}

pub fn remap_env(
    extra_env: &HashMap<String, String>,
    from_var: &str,
    to_key: &str,
    what: &str,
    docs: &str,
) -> Result<(String, String)> {
    let value = require_named_env(extra_env, from_var, what, docs)?;
    Ok((to_key.to_string(), value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plugin_name() {
        assert_eq!(parse_plugin_name("yubikey").unwrap(), "yubikey");
        assert!(parse_plugin_name("Yubi").is_err());
        assert!(parse_plugin_name("").is_err());
        assert!(
            parse_plugin_name("age-plugin-yubikey")
                .unwrap_err()
                .to_string()
                .contains("plugin=yubikey")
        );
    }

    #[test]
    fn test_split_scheme_path_query() {
        let (path, query) =
            split_scheme_path_query("age://CIPHER?plugin=yubikey&identity=YUBI_ID", "age").unwrap();
        assert_eq!(path, "CIPHER");
        assert_eq!(query.get("plugin"), Some("yubikey"));
        assert_eq!(query.get("identity"), Some("YUBI_ID"));
        assert!(split_scheme_path_query("age://?plugin=yubikey", "age").is_err());
        assert!(split_scheme_path_query("sops://secrets.enc.yaml", "age").is_err());
    }
}
