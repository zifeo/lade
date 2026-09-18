use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::FxHashMap;
use url::Url;

use crate::Hydration;

use super::{Provider, Warnings, add_url, json_query_string, require_cli_ok, run_cli, search_cli};

const DOCS: &str = "https://learn.microsoft.com/en-us/cli/azure/keyvault/secret";

// Microsoft data-plane suffixes: public, US Gov, China (21Vianet).
// https://learn.microsoft.com/en-us/azure/key-vault/general/about-keys-secrets-certificates
const CLOUDS: &[&str] = &[
    ".vault.azure.net",
    ".vault.usgovcloudapi.net",
    ".vault.azure.cn",
];

#[derive(Default)]
pub struct AzureKv {
    urls: FxHashMap<Url, String>,
}

impl AzureKv {
    pub fn new() -> Self {
        Default::default()
    }
}

fn vault_name(host: &str) -> Result<String> {
    if host.is_empty() {
        bail!("Azure Key Vault URI missing vault name");
    }
    for suffix in CLOUDS {
        if let Some(name) = host.strip_suffix(suffix) {
            if name.is_empty() || name.contains('.') {
                bail!("Azure Key Vault URI must be azurekv://<vault>/<name>");
            }
            return Ok(name.to_string());
        }
    }
    if host.contains('.') {
        bail!("Azure Key Vault URI must be a vault name or vault.vault.azure.net");
    }
    Ok(host.to_string())
}

fn vault_and_name(url: &Url) -> Result<(String, String)> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("Azure Key Vault URI missing vault name"))?;
    let vault = vault_name(host)?;
    let name = urlencoding::decode(url.path().trim_start_matches('/'))
        .map_err(|e| anyhow!("Azure Key Vault error: {e}"))?
        .into_owned();
    if name.is_empty() || name.contains('/') {
        bail!("Azure Key Vault URI must be azurekv://<vault>/<name>");
    }
    Ok((vault, name))
}

fn query(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == "query")
        .map(|(_, v)| v.into_owned())
}

fn secret_value(raw: &str, name: &str) -> Result<String> {
    let json: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| anyhow!("Azure Key Vault error: {e}. See {DOCS}."))?;
    json.get("value")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Azure Key Vault error: secret '{name}' has no value. See {DOCS}."))
}

#[async_trait]
impl Provider for AzureKv {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value.clone(), "azurekv")?;
        let url = Url::parse(&value)?;
        vault_and_name(&url)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Azure Key Vault"
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn search(&self, extra_env: &HashMap<String, String>) -> Result<Vec<String>> {
        let Some(vault) = extra_env.get("LADE_SEARCH_SCOPE") else {
            return Ok(Vec::new());
        };
        let output = search_cli(
            "az",
            &[
                "keyvault",
                "secret",
                "list",
                "--vault-name",
                vault,
                "--query",
                "[].name",
                "-o",
                "tsv",
            ],
            extra_env,
        )?;
        if !output.status.success() {
            return Err(anyhow!("az login required"));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }

    fn batch_unit(&self) -> &'static str {
        "(vault, name)"
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
        let extra_env = Arc::new(extra_env.clone());
        let name = self.name();
        let install_url = self.install_url();
        let mut by_secret: HashMap<(String, String), Vec<(String, Option<String>)>> =
            HashMap::new();
        for (url, raw) in &self.urls {
            let (vault, secret) = vault_and_name(url)?;
            by_secret
                .entry((vault, secret))
                .or_default()
                .push((raw.clone(), query(url)));
        }
        let fetches = by_secret.into_iter().map(|((vault, secret), uris)| {
            let extra_env = Arc::clone(&extra_env);
            async move {
                let output = run_cli(
                    &[
                        "az",
                        "keyvault",
                        "secret",
                        "show",
                        "--vault-name",
                        &vault,
                        "--name",
                        &secret,
                        "-o",
                        "json",
                    ],
                    &extra_env,
                    name,
                    install_url,
                    None,
                )
                .await?;
                require_cli_ok(&output, name)?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let value = secret_value(&stdout, &secret)?;
                let mut hydration = Hydration::default();
                for (raw, q) in uris {
                    hydration.insert(raw, json_query_string(&value, q.as_deref())?);
                }
                Ok::<_, anyhow::Error>(hydration)
            }
        });
        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }
}

pub(super) const SEARCH_SCOPE: super::add::AddField = super::add::AddField {
    key: "vault",
    prompt: "Vault name: ",
    default: None,
};

pub(super) const ADD_FIELDS: &[super::add::AddField] = &[];

pub(super) fn compose_add_uri(picked: &str, extras: &HashMap<String, String>) -> String {
    let vault = extras.get("vault").map(String::as_str).unwrap_or("vault");
    format!("azurekv://{vault}/{picked}")
}

#[cfg(test)]
mod tests;
