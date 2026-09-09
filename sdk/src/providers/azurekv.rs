use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::FxHashMap;
use url::Url;

use crate::Hydration;

use super::{Provider, Transport, Warnings, add_url, env_lookup, json_query_string};

const DOCS: &str =
    "https://learn.microsoft.com/en-us/rest/api/keyvault/secrets/get-secret/get-secret";

const CLOUDS: &[(&str, &str)] = &[
    (".vault.azure.net", "https://vault.azure.net/.default"),
    (
        ".vault.usgovcloudapi.net",
        "https://vault.usgovcloudapi.net/.default",
    ),
    (".vault.azure.cn", "https://vault.azure.cn/.default"),
];

#[async_trait]
pub(crate) trait AzureKvApi: Send + Sync {
    async fn get(&self, vault_host: &str, name: &str) -> Result<String>;
}

struct RestAzureKv {
    token: String,
    client: reqwest::Client,
}

#[async_trait]
impl AzureKvApi for RestAzureKv {
    async fn get(&self, vault_host: &str, name: &str) -> Result<String> {
        let url = format!("https://{vault_host}/secrets/{name}?api-version=7.4");
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| anyhow!("Azure Key Vault error: {e}. See {DOCS}."))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| anyhow!("Azure Key Vault error: {e}. See {DOCS}."))?;
        if !status.is_success() {
            bail!("Azure Key Vault error: {status} {body}. See {DOCS}.");
        }
        let json: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| anyhow!("Azure Key Vault error: {e}"))?;
        json.get("value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                anyhow!("Azure Key Vault error: secret '{name}' has no value. See {DOCS}.")
            })
    }
}

#[derive(Default)]
pub struct AzureKv {
    urls: FxHashMap<Url, String>,
    api: Option<Arc<dyn AzureKvApi>>,
}

impl AzureKv {
    pub fn new() -> Self {
        Default::default()
    }

    #[cfg(test)]
    pub(crate) fn with_api(api: Arc<dyn AzureKvApi>) -> Self {
        Self {
            urls: FxHashMap::default(),
            api: Some(api),
        }
    }
}

fn vault_endpoint(host: &str) -> Result<(String, &'static str)> {
    if host.is_empty() {
        bail!("Azure Key Vault URI missing vault name");
    }
    for (suffix, scope) in CLOUDS {
        if let Some(name) = host.strip_suffix(suffix) {
            if name.is_empty() || name.contains('.') {
                bail!("Azure Key Vault URI must be azurekv://<vault>/<name>");
            }
            return Ok((format!("{name}{suffix}"), *scope));
        }
    }
    if host.contains('.') {
        bail!("Azure Key Vault URI must be a vault name or vault.vault.azure.net");
    }
    Ok((
        format!("{host}.vault.azure.net"),
        "https://vault.azure.net/.default",
    ))
}

fn vault_and_name(url: &Url) -> Result<(String, String, &'static str)> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("Azure Key Vault URI missing vault name"))?;
    let (vault_host, scope) = vault_endpoint(host)?;
    let name = urlencoding::decode(url.path().trim_start_matches('/'))
        .map_err(|e| anyhow!("Azure Key Vault error: {e}"))?
        .into_owned();
    if name.is_empty() || name.contains('/') {
        bail!("Azure Key Vault URI must be azurekv://<vault>/<name>");
    }
    Ok((vault_host, name, scope))
}

fn query(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == "query")
        .map(|(_, v)| v.into_owned())
}

async fn token_from(
    credential: &dyn azure_core::credentials::TokenCredential,
    scopes: &[&str],
) -> Result<String, String> {
    credential
        .get_token(scopes, None)
        .await
        .map(|token| token.token.secret().to_string())
        .map_err(|error| error.to_string())
}

async fn token(extra_env: &HashMap<String, String>, scope: &str) -> Result<String> {
    if let Some(token) = env_lookup(extra_env, &["AZURE_ACCESS_TOKEN", "LADE_AZURE_TOKEN"]) {
        return Ok(token);
    }
    use azure_core::credentials::Secret;
    let scopes = [scope];
    let mut errors = Vec::new();
    if let (Some(tenant), Some(client_id), Some(secret)) = (
        env_lookup(extra_env, &["AZURE_TENANT_ID"]),
        env_lookup(extra_env, &["AZURE_CLIENT_ID"]),
        env_lookup(extra_env, &["AZURE_CLIENT_SECRET"]),
    ) {
        match azure_identity::ClientSecretCredential::new(
            &tenant,
            client_id,
            Secret::new(secret),
            None,
        ) {
            Ok(credential) => match token_from(credential.as_ref(), &scopes).await {
                Ok(token) => return Ok(token),
                Err(error) => errors.push(error),
            },
            Err(error) => errors.push(error.to_string()),
        }
    }
    match azure_identity::DeveloperToolsCredential::new(None) {
        Ok(credential) => match token_from(credential.as_ref(), &scopes).await {
            Ok(token) => return Ok(token),
            Err(error) => errors.push(error),
        },
        Err(error) => errors.push(error.to_string()),
    }
    match azure_identity::ManagedIdentityCredential::new(None) {
        Ok(credential) => match token_from(credential.as_ref(), &scopes).await {
            Ok(token) => return Ok(token),
            Err(error) => errors.push(error),
        },
        Err(error) => errors.push(error.to_string()),
    }
    bail!(
        "Azure Key Vault error: {}. See {DOCS}.",
        if errors.is_empty() {
            "no credential succeeded".to_string()
        } else {
            errors.join("; ")
        }
    )
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

    fn transport(&self) -> Transport {
        Transport::Sdk
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
        let mut scope_for_host: HashMap<String, &'static str> = HashMap::new();
        for (url, raw) in &self.urls {
            let (vault_host, name, scope) = vault_and_name(url)?;
            scope_for_host.insert(vault_host.clone(), scope);
            by_secret
                .entry((vault_host, name))
                .or_default()
                .push((raw.clone(), query(url)));
        }
        let api: Arc<dyn AzureKvApi> = if let Some(api) = &self.api {
            Arc::clone(api)
        } else {
            let scope = scope_for_host
                .values()
                .next()
                .copied()
                .unwrap_or("https://vault.azure.net/.default");
            if scope_for_host.values().any(|s| *s != scope)
                && env_lookup(extra_env, &["AZURE_ACCESS_TOKEN", "LADE_AZURE_TOKEN"]).is_none()
            {
                bail!(
                    "Azure Key Vault cannot mix sovereign clouds in one resolve without AZURE_ACCESS_TOKEN. See {DOCS}."
                );
            }
            Arc::new(RestAzureKv {
                token: token(extra_env, scope).await?,
                client: reqwest::Client::new(),
            })
        };
        let fetches = by_secret.into_iter().map(|((vault, name), uris)| {
            let api = Arc::clone(&api);
            async move {
                let value = api.get(&vault, &name).await?;
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

#[cfg(test)]
mod tests;
