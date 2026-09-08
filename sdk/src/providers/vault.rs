use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use url::Url;

use crate::Hydration;

use super::{Provider, Transport, Warnings, add_url, env_lookup, host_with_port};

const DOCS: &str = "https://developer.hashicorp.com/vault/docs/secrets/kv/kv-v2";

#[derive(Default)]
pub struct Vault {
    urls: FxHashMap<Url, String>,
}

impl Vault {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Deserialize)]
struct VaultGetKVData {
    data: HashMap<String, String>,
}

#[derive(Deserialize)]
struct VaultExport {
    data: VaultGetKVData,
}

fn path_parts(url: &Url) -> Result<(String, String, String)> {
    let mount = url
        .path()
        .split('/')
        .nth(1)
        .ok_or_else(|| anyhow!("Vault URI missing mount"))?;
    let key = url
        .path()
        .split('/')
        .nth(2)
        .ok_or_else(|| anyhow!("Vault URI missing key"))?;
    let field = url
        .path()
        .split('/')
        .nth(3)
        .ok_or_else(|| anyhow!("Vault URI missing field"))?;
    Ok((
        urlencoding::decode(mount)?.into_owned(),
        urlencoding::decode(key)?.into_owned(),
        urlencoding::decode(field)?.into_owned(),
    ))
}

fn vault_scheme(extra_env: &HashMap<String, String>) -> &'static str {
    if extra_env.contains_key("LADE_VAULT_HTTP") || std::env::var("LADE_VAULT_HTTP").is_ok() {
        "http"
    } else {
        "https"
    }
}

fn token_from_home(home: &str) -> Option<String> {
    let raw = std::fs::read_to_string(PathBuf::from(home).join(".vault-token")).ok()?;
    let token = raw.trim();
    (!token.is_empty()).then(|| token.to_string())
}

fn vault_token(extra_env: &HashMap<String, String>) -> Option<String> {
    for key in ["VAULT_TOKEN", "LADE_VAULT_TOKEN"] {
        if let Some(value) = extra_env.get(key)
            && !value.is_empty()
        {
            return Some(value.clone());
        }
    }
    if let Some(home) = extra_env.get("HOME") {
        return token_from_home(home);
    }
    env_lookup(extra_env, &["VAULT_TOKEN", "LADE_VAULT_TOKEN"]).or_else(|| {
        std::env::var("HOME")
            .ok()
            .and_then(|home| token_from_home(&home))
    })
}

async fn fetch_secret(
    client: &reqwest::Client,
    address: &str,
    token: &str,
    namespace: Option<&str>,
    mount: &str,
    key: &str,
) -> Result<HashMap<String, String>> {
    let url = format!("{address}/v1/{mount}/data/{key}");
    let mut req = client.get(&url).header("X-Vault-Token", token);
    if let Some(namespace) = namespace {
        req = req.header("X-Vault-Namespace", namespace);
    }
    let response = req.send().await.map_err(|e| anyhow!("Vault error: {e}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| anyhow!("Vault error: {e}"))?;
    if !status.is_success() {
        bail!("Vault error: {status} {body}");
    }
    let loaded: VaultExport =
        serde_json::from_str(&body).map_err(|e| anyhow!("Vault error: {e}"))?;
    Ok(loaded.data.data)
}

#[async_trait]
impl Provider for Vault {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value, "vault")
    }

    fn name(&self) -> &'static str {
        "Vault"
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn transport(&self) -> Transport {
        Transport::Sdk
    }

    fn batch_unit(&self) -> &'static str {
        "(host, mount, key)"
    }

    fn has_work(&self) -> bool {
        !self.urls.is_empty()
    }

    async fn resolve(
        &self,
        _: &Path,
        extra_env: &HashMap<String, String>,
        _: &Warnings,
    ) -> Result<Hydration> {
        let extra_env = extra_env.clone();
        let token = vault_token(&extra_env).ok_or_else(|| {
            anyhow!(
                "Vault token not set. Set VAULT_TOKEN or LADE_VAULT_TOKEN, or run vault login. See {DOCS}."
            )
        })?;
        let namespace = env_lookup(&extra_env, &["VAULT_NAMESPACE", "LADE_VAULT_NAMESPACE"]);
        let scheme = vault_scheme(&extra_env);
        let client = reqwest::Client::new();

        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|(url, _)| host_with_port(url))
            .into_iter()
            .flat_map(|(host, group)| {
                group
                    .into_iter()
                    .into_group_map_by(|(url, _)| {
                        url.path().split('/').nth(1).unwrap_or("").to_string()
                    })
                    .into_iter()
                    .flat_map(|(mount, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|(url, _)| {
                                url.path().split('/').nth(2).unwrap_or("").to_string()
                            })
                            .into_iter()
                            .map(|(key, group)| {
                                let host = host.clone();
                                let mount = mount.clone();
                                let token = token.clone();
                                let namespace = namespace.clone();
                                let client = client.clone();
                                async move {
                                    let address = format!("{scheme}://{host}");
                                    let mount = urlencoding::decode(&mount)
                                        .map_err(|e| anyhow!("Vault error: {e}"))?
                                        .into_owned();
                                    let key = urlencoding::decode(&key)
                                        .map_err(|e| anyhow!("Vault error: {e}"))?
                                        .into_owned();
                                    let loaded = fetch_secret(
                                        &client,
                                        &address,
                                        &token,
                                        namespace.as_deref(),
                                        &mount,
                                        &key,
                                    )
                                    .await?;
                                    let mut hydration = Hydration::default();
                                    for (url, value) in group {
                                        let (_, _, field) = path_parts(url)?;
                                        let secret = loaded.get(&field).ok_or_else(|| {
                                            anyhow!("Variable not found in Vault: {field}")
                                        })?;
                                        hydration.insert(value.clone(), secret.clone());
                                    }
                                    Ok::<_, anyhow::Error>(hydration)
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }
}

#[cfg(test)]
mod tests;
