use std::{collections::HashMap, path::Path};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use rustc_hash::FxHashMap;
use url::Url;

use crate::Hydration;

use super::{Provider, Warnings, add_url, fetch_grouped, query_param, search_cli_ok};

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
        let output = search_cli_ok(
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
            "az login required",
        )?;
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
        let mut by_secret: HashMap<(String, String), Vec<(String, Option<String>)>> =
            HashMap::new();
        for (url, raw) in &self.urls {
            let (vault, secret) = vault_and_name(url)?;
            by_secret
                .entry((vault, secret))
                .or_default()
                .push((raw.clone(), query_param(url, "query")));
        }
        fetch_grouped(
            by_secret,
            extra_env,
            self.name(),
            self.install_url(),
            |(vault, secret)| {
                vec![
                    "az".to_string(),
                    "keyvault".to_string(),
                    "secret".to_string(),
                    "show".to_string(),
                    "--vault-name".to_string(),
                    vault.clone(),
                    "--name".to_string(),
                    secret.clone(),
                    "-o".to_string(),
                    "json".to_string(),
                ]
            },
            |stdout, (_, secret)| secret_value(stdout, secret),
        )
        .await
    }

    fn compose_add_uri(&self, picked: &str, extras: &HashMap<String, String>) -> Option<String> {
        let vault = extras.get("vault").map(String::as_str).unwrap_or("vault");
        Some(format!("azurekv://{vault}/{picked}"))
    }
}

pub(super) const SEARCH_SCOPE: super::add::AddField = super::add::AddField {
    key: "vault",
    prompt: "Vault name: ",
    default: None,
};

pub(super) const ADD_FIELDS: &[super::add::AddField] = &[];

#[cfg(test)]
mod tests;
