use std::{collections::HashMap, path::Path, path::PathBuf};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use url::Url;

use crate::Hydration;

use super::{
    Provider, Transport, Warnings, add_url, decode_seg, env_lookup, host_with_port, search_cli_ok,
};

const DOCS: &str = "https://developer.hashicorp.com/vault/docs/commands/kv/get";

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

struct VaultPath {
    mount: String,
    key: String,
    field: String,
}

impl VaultPath {
    fn uri(&self, host: &str) -> String {
        format!("vault://{host}/{}/{}/{}", self.mount, self.key, self.field)
    }
}

fn path_parts(url: &Url) -> Result<VaultPath> {
    let segs = url
        .path_segments()
        .ok_or_else(|| anyhow!("Vault URI missing mount"))?;
    let decoded = segs
        .map(|seg| decode_seg(seg, "vault"))
        .collect::<Result<Vec<_>>>()?;
    let mount = decoded
        .first()
        .filter(|s| !s.is_empty())
        .cloned()
        .ok_or_else(|| anyhow!("Vault URI missing mount"))?;
    let field = decoded
        .last()
        .cloned()
        .ok_or_else(|| anyhow!("Vault URI missing field"))?;
    if decoded.len() < 3 {
        return Err(anyhow!("Vault URI missing key"));
    }
    let key = decoded[1..decoded.len() - 1].join("/");
    if key.is_empty() {
        return Err(anyhow!("Vault URI missing key"));
    }
    if field.is_empty() {
        return Err(anyhow!("Vault URI missing field"));
    }
    Ok(VaultPath { mount, key, field })
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

fn kv_v2_url(address: &str, mount: &str, key: &str) -> String {
    let mount = urlencoding::encode(mount);
    let key = key
        .split('/')
        .map(urlencoding::encode)
        .collect::<Vec<_>>()
        .join("/");
    format!("{address}/v1/{mount}/data/{key}")
}

async fn kv_get(
    client: &reqwest::Client,
    address: &str,
    token: &str,
    namespace: Option<&str>,
    mount: &str,
    key: &str,
) -> Result<VaultExport> {
    let url = kv_v2_url(address, mount, key);
    let mut req = client.get(&url).header("X-Vault-Token", token);
    if let Some(namespace) = namespace {
        req = req.header("X-Vault-Namespace", namespace);
    }
    let response = req.send().await.map_err(|e| anyhow!("Vault error: {e}"))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| anyhow!("Vault error: {e}"))?;
    if !status.is_success() {
        return Err(anyhow!(
            "Vault error: HTTP {status}: {}",
            String::from_utf8_lossy(&bytes)
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| anyhow!("Vault error: {e} (stderr: )"))
}

#[async_trait]
impl Provider for Vault {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value, "vault")
    }

    fn timeout(&self) -> std::time::Duration {
        super::PROVIDER_TIMEOUT
    }

    fn name(&self) -> &'static str {
        "Vault"
    }

    fn transport(&self) -> Transport {
        Transport::Sdk
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn search(&self, extra_env: &HashMap<String, String>) -> Result<Vec<String>> {
        let output = search_cli_ok(
            "vault",
            &["kv", "list", "-format=json", "secret"],
            extra_env,
            self.timeout(),
            "vault login required",
        )?;
        let paths: Vec<String> = match serde_json::from_slice::<Vec<String>>(&output.stdout) {
            Ok(json) => json,
            Err(_) => String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with("Keys"))
                .map(str::to_string)
                .collect(),
        };
        Ok(paths)
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
        let client = reqwest::Client::builder()
            .timeout(self.timeout())
            .build()
            .map_err(|e| anyhow!("Vault error: {e}"))?;

        let parsed = self
            .urls
            .iter()
            .map(|(url, value)| {
                let parts = path_parts(url)?;
                Ok::<_, anyhow::Error>((url, value, parts))
            })
            .collect::<Result<Vec<_>>>()?;

        let fetches = parsed
            .into_iter()
            .into_group_map_by(|(url, _, _)| host_with_port(url))
            .into_iter()
            .flat_map(|(host, group)| {
                group
                    .into_iter()
                    .into_group_map_by(|(_, _, parts)| (parts.mount.clone(), parts.key.clone()))
                    .into_iter()
                    .map(|((mount, key), group)| {
                        let host = host.clone();
                        let token = token.clone();
                        let namespace = namespace.clone();
                        let client = client.clone();
                        async move {
                            let address = format!("{scheme}://{host}");
                            let loaded = kv_get(
                                &client,
                                &address,
                                &token,
                                namespace.as_deref(),
                                &mount,
                                &key,
                            )
                            .await?;
                            let mut hydration = Hydration::default();
                            for (_url, value, parts) in group {
                                let secret =
                                    loaded.data.data.get(&parts.field).ok_or_else(|| {
                                        anyhow!("Variable not found in Vault: {}", parts.field)
                                    })?;
                                hydration.insert(value.clone(), secret.clone());
                            }
                            Ok::<_, anyhow::Error>(hydration)
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }

    fn compose_add_uri(&self, picked: &str, extras: &HashMap<String, String>) -> Option<String> {
        let field = extras
            .get("field")
            .map(String::as_str)
            .unwrap_or("password");
        let host = extras
            .get("host")
            .map(String::as_str)
            .unwrap_or("127.0.0.1:8200");
        let url =
            Url::parse(&format!("vault://{host}/secret/{picked}/{field}")).expect("vault add URI");
        Some(path_parts(&url).expect("vault add URI path").uri(host))
    }
}

pub(super) const ADD_FIELDS: &[super::add::AddField] = &[
    super::add::AddField {
        key: "field",
        prompt: "Field (password): ",
        default: Some("password"),
    },
    super::add::AddField {
        key: "host",
        prompt: "Vault host (127.0.0.1:8200): ",
        default: Some("127.0.0.1:8200"),
    },
];

#[cfg(test)]
mod tests;
