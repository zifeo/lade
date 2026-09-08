use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::FxHashMap;

use crate::Hydration;

use super::params::{
    Query, apply_process_path, parse_plugin_name, remap_env, require_age_plugin_on_path,
    require_identity_names, split_scheme_path_query,
};
use super::{Provider, Transport, Warnings, json_query_string, run_cli};

const DOCS: &str = "https://github.com/getsops/sops";

const SOPS_KEYS: &[&str] = &["age", "pgp", "aws_kms", "gcp_kms", "azure_kv", "hc_vault"];

#[derive(Default)]
pub struct Sops {
    values: FxHashMap<String, String>,
}

impl Sops {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SopsUri {
    path: String,
    field: Option<String>,
    plugin: Option<String>,
    query: Query,
}

fn parse_sops(value: &str) -> Result<SopsUri> {
    let (path, query) = split_scheme_path_query(value, "sops")?;
    let plugin = match query.get("plugin") {
        Some(name) => Some(parse_plugin_name(name)?),
        None => None,
    };
    if let Some(plugin) = &plugin
        && !SOPS_KEYS.contains(&plugin.as_str())
    {
        require_identity_names(&query, Some(plugin), DOCS)?;
    }
    if plugin.as_deref() == Some("age") {
        require_identity_names(&query, Some("age"), DOCS)?;
    }
    Ok(SopsUri {
        path: path.to_string(),
        field: query.get("query").map(str::to_string),
        plugin,
        query,
    })
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn resolve_path(cwd: &Path, raw: &str) -> Result<PathBuf> {
    let decoded = urlencoding::decode(raw)
        .map_err(|e| anyhow!("SOPS error: {e}"))?
        .into_owned();
    let user = directories::UserDirs::new().ok_or_else(|| anyhow!("cannot get HOME location"))?;
    let path = if let Some(rest) = decoded.strip_prefix("~/") {
        user.home_dir().join(rest)
    } else if let Some(rest) = decoded.strip_prefix("$HOME/") {
        user.home_dir().join(rest)
    } else {
        cwd.join(&decoded)
    };
    let joined = if path.starts_with(PathBuf::from_str("/")?) {
        path
    } else {
        cwd.join(path)
    };
    Ok(normalize_path(joined))
}

fn child_env(
    extra_env: &HashMap<String, String>,
    uri: &SopsUri,
) -> Result<HashMap<String, String>> {
    let mut env = extra_env.clone();
    let Some(plugin) = uri.plugin.as_deref() else {
        return Ok(env);
    };
    if SOPS_KEYS.contains(&plugin) {
        apply_sops_key(&mut env, extra_env, plugin, &uri.query)?;
        return Ok(env);
    }
    require_age_plugin_on_path(extra_env, plugin, DOCS)?;
    apply_sops_key(&mut env, extra_env, "age", &uri.query)?;
    Ok(env)
}

fn apply_sops_key(
    env: &mut HashMap<String, String>,
    extra_env: &HashMap<String, String>,
    plugin: &str,
    query: &Query,
) -> Result<()> {
    match plugin {
        "age" => {
            if let Some(var) = query.get("identity") {
                let (k, v) = remap_env(extra_env, var, "SOPS_AGE_KEY", "SOPS", DOCS)?;
                env.insert(k, v);
            }
            if let Some(var) = query.get("identity_file") {
                let (k, v) = remap_env(extra_env, var, "SOPS_AGE_KEY_FILE", "SOPS", DOCS)?;
                env.insert(k, v);
            }
        }
        "aws_kms" => {
            if let Some(var) = query.get("profile") {
                let (k, v) = remap_env(extra_env, var, "AWS_PROFILE", "SOPS", DOCS)?;
                env.insert(k, v);
            }
            if let Some(var) = query.get("region") {
                let (k, v) = remap_env(extra_env, var, "AWS_REGION", "SOPS", DOCS)?;
                env.insert(k, v);
            }
        }
        "gcp_kms" => {
            if let Some(var) = query.get("credentials") {
                let (k, v) = remap_env(
                    extra_env,
                    var,
                    "GOOGLE_APPLICATION_CREDENTIALS",
                    "SOPS",
                    DOCS,
                )?;
                env.insert(k, v);
            }
        }
        "azure_kv" => {
            if let Some(var) = query.get("token") {
                let (k, v) = remap_env(extra_env, var, "AZURE_ACCESS_TOKEN", "SOPS", DOCS)?;
                env.insert(k, v);
            }
        }
        "hc_vault" => {
            if let Some(var) = query.get("token") {
                let (k, v) = remap_env(extra_env, var, "VAULT_TOKEN", "SOPS", DOCS)?;
                env.insert(k, v);
            }
        }
        "pgp" => {
            if let Some(var) = query.get("homedir") {
                let (k, v) = remap_env(extra_env, var, "GNUPGHOME", "SOPS", DOCS)?;
                env.insert(k, v);
            }
        }
        other => bail!("SOPS unknown plugin {other}. See {DOCS}."),
    }
    Ok(())
}

fn credential_key(uri: &SopsUri) -> String {
    format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        uri.plugin.as_deref().unwrap_or(""),
        uri.query.get("identity").unwrap_or(""),
        uri.query.get("identity_file").unwrap_or(""),
        uri.query.get("profile").unwrap_or(""),
        uri.query.get("region").unwrap_or(""),
        uri.query.get("credentials").unwrap_or(""),
        uri.query.get("token").unwrap_or(""),
        uri.query.get("homedir").unwrap_or(""),
    )
}

#[async_trait]
impl Provider for Sops {
    fn add(&mut self, value: String) -> Result<()> {
        let rest = value
            .strip_prefix("sops://")
            .ok_or_else(|| anyhow!("Not a sops scheme"))?;
        if rest.is_empty() || rest.starts_with('?') {
            bail!("sops:// path cannot be empty");
        }
        self.values.insert(value.clone(), value);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "SOPS"
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn transport(&self) -> Transport {
        Transport::Cli
    }

    fn batch_unit(&self) -> &'static str {
        "(path, plugin, identity)"
    }

    fn has_work(&self) -> bool {
        !self.values.is_empty()
    }

    async fn resolve(
        &self,
        cwd: &Path,
        extra_env: &HashMap<String, String>,
        _: &Warnings,
    ) -> Result<Hydration> {
        apply_process_path(extra_env);
        let name = self.name();
        let install_url = self.install_url();
        let mut by_group: HashMap<String, Vec<SopsUri>> = HashMap::new();
        let mut raws: HashMap<String, Vec<String>> = HashMap::new();
        for raw in self.values.keys() {
            let uri = parse_sops(raw)?;
            let file = resolve_path(cwd, &uri.path)?;
            let key = format!("{}\0{}", file.to_string_lossy(), credential_key(&uri));
            by_group.entry(key.clone()).or_default().push(uri);
            raws.entry(key).or_default().push(raw.clone());
        }
        let fetches = by_group.into_iter().map(|(key, group)| {
            let extra_env = extra_env.clone();
            let cwd = cwd.to_path_buf();
            let raws = raws.remove(&key).unwrap_or_default();
            async move {
                let first = group.first().expect("sops group is not empty");
                let file = resolve_path(&cwd, &first.path)?;
                let env = child_env(&extra_env, first)?;
                let cmd = [
                    "sops",
                    "--decrypt",
                    "--output-type",
                    "json",
                    file.to_str()
                        .ok_or_else(|| anyhow!("SOPS error: path is not UTF-8"))?,
                ];
                let output = run_cli(&cmd, &env, name, install_url, Some(&cwd)).await?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!("SOPS error: {stderr}. See {DOCS}.");
                }
                let body = String::from_utf8(output.stdout)
                    .map_err(|e| anyhow!("SOPS error: {e}. See {DOCS}."))?;
                let mut hydration = Hydration::default();
                for (uri, raw) in group.into_iter().zip(raws) {
                    hydration.insert(raw, json_query_string(&body, uri.field.as_deref())?);
                }
                Ok::<_, anyhow::Error>(hydration)
            }
        });
        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }
}

#[cfg(test)]
mod tests;
