use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use std::{collections::HashMap, path::Path};
use url::Url;

use crate::Hydration;

use super::{Provider, Transport, Warnings, add_url, env_lookup, host_with_port};

const DOCS: &str = "https://infisical.com/docs/api-reference/overview/introduction";

#[derive(Default)]
pub struct Infisical {
    urls: FxHashMap<Url, String>,
}

impl Infisical {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Deserialize)]
struct InfisicalSecret {
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(alias = "secretKey", default)]
    secret_key: Option<String>,
    #[serde(alias = "secretValue", default)]
    secret_value: Option<String>,
}

impl InfisicalSecret {
    fn pair(self) -> Option<(String, String)> {
        let key = self.secret_key.or(self.key)?;
        let value = self.secret_value.or(self.value)?;
        Some((key, value))
    }
}

#[derive(Deserialize)]
struct InfisicalList {
    #[serde(default)]
    secrets: Vec<InfisicalSecret>,
}

fn decode_seg(raw: &str) -> Result<String> {
    urlencoding::decode(raw)
        .map_err(|e| anyhow!("invalid percent-encoding in infisical:// URL: {e}"))
        .map(|s| s.into_owned())
}

fn secret_path_and_var(url: &Url) -> Result<(String, String)> {
    let segs: Vec<&str> = url.path().split('/').collect();
    if segs.len() < 4 || segs.last().is_some_and(|s| s.is_empty()) {
        bail!("Infisical URI must be infisical://DOMAIN/PROJECT_ID/ENV_NAME/SECRET_NAME");
    }
    let variable = decode_seg(segs.last().ok_or_else(|| anyhow!("Missing variable"))?)?;
    if variable.is_empty() {
        bail!("Infisical URI must be infisical://DOMAIN/PROJECT_ID/ENV_NAME/SECRET_NAME");
    }
    let path = if segs.len() > 4 {
        let folders = segs[3..segs.len() - 1]
            .iter()
            .map(|s| decode_seg(s))
            .collect::<Result<Vec<_>>>()?;
        format!("/{}", folders.join("/"))
    } else {
        "/".to_string()
    };
    Ok((path, variable))
}

async fn export_path(
    client: &reqwest::Client,
    host: &str,
    token: &str,
    project: &str,
    env: &str,
    path: &str,
    extra_env: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let domain = if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else if extra_http(host, extra_env) {
        format!("http://{host}")
    } else {
        format!("https://{host}")
    };
    let url = format!("{domain}/api/v3/secrets/raw");
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .query(&[
            ("workspaceId", project),
            ("environment", env),
            ("secretPath", path),
        ])
        .send()
        .await
        .map_err(|e| anyhow!("Infisical error: {e}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| anyhow!("Infisical error: {e}"))?;
    if !status.is_success() {
        bail!("Infisical error: {status} {body}. See {DOCS}.");
    }
    if let Ok(list) = serde_json::from_str::<InfisicalList>(&body) {
        return Ok(list.secrets.into_iter().filter_map(|s| s.pair()).collect());
    }
    let rows: Vec<InfisicalSecret> =
        serde_json::from_str(&body).map_err(|e| anyhow!("Infisical error: {e} ({body})"))?;
    Ok(rows.into_iter().filter_map(|s| s.pair()).collect())
}

fn extra_http(host: &str, extra_env: &HashMap<String, String>) -> bool {
    extra_env.contains_key("LADE_INFISICAL_HTTP")
        || std::env::var("LADE_INFISICAL_HTTP").is_ok()
        || host.starts_with("127.0.0.1")
        || host.starts_with("localhost")
}

#[async_trait]
impl Provider for Infisical {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value.clone(), "infisical")?;
        let url = Url::parse(&value)?;
        secret_path_and_var(&url)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Infisical"
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn transport(&self) -> Transport {
        Transport::Sdk
    }

    fn batch_unit(&self) -> &'static str {
        "(host, project, env, path)"
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
        let token = env_lookup(
            extra_env,
            &["INFISICAL_TOKEN", "INFISICAL_API_TOKEN", "LADE_INFISICAL_TOKEN"],
        )
        .ok_or_else(|| {
            anyhow!(
                "Infisical token not set. Set INFISICAL_TOKEN, INFISICAL_API_TOKEN, or LADE_INFISICAL_TOKEN. See {DOCS}."
            )
        })?;
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
                    .flat_map(|(project, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|(url, _)| {
                                url.path().split('/').nth(2).unwrap_or("").to_string()
                            })
                            .into_iter()
                            .map(|(env, group)| {
                                let prepared = group
                                    .iter()
                                    .map(|(url, value)| {
                                        let (path, variable) = secret_path_and_var(url)?;
                                        Ok((path, variable, (*value).clone()))
                                    })
                                    .collect::<Result<Vec<_>>>();
                                let host = host.clone();
                                let project = project.clone();
                                let token = token.clone();
                                let client = client.clone();
                                let extra_env = extra_env.clone();
                                async move {
                                    let prepared = prepared?;
                                    let path_groups = prepared
                                        .into_iter()
                                        .into_group_map_by(|(path, _, _)| path.clone())
                                        .into_iter()
                                        .map(|(path, vars)| {
                                            (
                                                path,
                                                vars.into_iter()
                                                    .map(|(_, var, url)| (var, url))
                                                    .collect::<Vec<_>>(),
                                            )
                                        })
                                        .collect::<HashMap<_, _>>();
                                    let path_futures = path_groups.into_iter().map(
                                        |(path, variables)| {
                                            let host = host.clone();
                                            let project = project.clone();
                                            let env = env.clone();
                                            let token = token.clone();
                                            let client = client.clone();
                                            let extra_env = extra_env.clone();
                                            async move {
                                                let loaded = export_path(
                                                    &client,
                                                    &host,
                                                    &token,
                                                    &project,
                                                    &env,
                                                    &path,
                                                    &extra_env,
                                                )
                                                .await?;
                                                let mut missing_vars = Vec::new();
                                                let mut partial = Hydration::default();
                                                for (var_name, original_url) in variables {
                                                    if let Some(value) = loaded.get(&var_name) {
                                                        partial
                                                            .insert(original_url, value.clone());
                                                    } else {
                                                        missing_vars.push(var_name);
                                                    }
                                                }
                                                if !missing_vars.is_empty() {
                                                    bail!(
                                                        "Variables {} not found in path {} of Infisical project {}",
                                                        missing_vars.join(", "),
                                                        path,
                                                        project
                                                    );
                                                }
                                                Ok(partial)
                                            }
                                        },
                                    );
                                    let hydration: Hydration = try_join_all(path_futures)
                                        .await?
                                        .into_iter()
                                        .flatten()
                                        .collect();
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
