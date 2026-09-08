use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::FxHashMap;
use url::Url;

use crate::Hydration;

use super::{Provider, Transport, Warnings, add_url, env_lookup, json_query_string};

const DOCS: &str = "https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets.versions/access";

#[async_trait]
pub(crate) trait GcpSmApi: Send + Sync {
    async fn access(&self, project: &str, name: &str, location: Option<&str>) -> Result<String>;
}

struct RestGcpSm {
    token: String,
    client: reqwest::Client,
}

#[async_trait]
impl GcpSmApi for RestGcpSm {
    async fn access(&self, project: &str, name: &str, location: Option<&str>) -> Result<String> {
        let url = match location {
            Some(location) => format!(
                "https://secretmanager.{location}.rep.googleapis.com/v1/projects/{project}/locations/{location}/secrets/{name}/versions/latest:access"
            ),
            None => format!(
                "https://secretmanager.googleapis.com/v1/projects/{project}/secrets/{name}/versions/latest:access"
            ),
        };
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| anyhow!("GCP Secret Manager error: {e}. See {DOCS}."))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| anyhow!("GCP Secret Manager error: {e}. See {DOCS}."))?;
        if !status.is_success() {
            bail!("GCP Secret Manager error: {status} {body}. See {DOCS}.");
        }
        let json: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| anyhow!("GCP Secret Manager error: {e}"))?;
        let b64 = json
            .pointer("/payload/data")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!("GCP Secret Manager error: secret '{name}' has no payload. See {DOCS}.")
            })?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .map_err(|e| anyhow!("GCP Secret Manager error: {e}"))?;
        String::from_utf8(bytes).map_err(|_| {
            anyhow!("GCP Secret Manager error: secret '{name}' is not UTF-8. See {DOCS}.")
        })
    }
}

#[derive(Default)]
pub struct GcpSm {
    urls: FxHashMap<Url, String>,
    api: Option<Arc<dyn GcpSmApi>>,
}

impl GcpSm {
    pub fn new() -> Self {
        Default::default()
    }

    #[cfg(test)]
    pub(crate) fn with_api(api: Arc<dyn GcpSmApi>) -> Self {
        Self {
            urls: FxHashMap::default(),
            api: Some(api),
        }
    }
}

fn project_and_name(url: &Url) -> Result<(String, String, Option<String>)> {
    let project = url
        .host_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("GCP Secret Manager URI missing project"))?;
    let name = urlencoding::decode(url.path().trim_start_matches('/'))
        .map_err(|e| anyhow!("GCP Secret Manager error: {e}"))?
        .into_owned();
    if name.is_empty() || name.contains('/') {
        bail!("GCP Secret Manager URI must be gcpsm://<project>/<name>");
    }
    let location = url
        .query_pairs()
        .find(|(k, _)| k == "location")
        .map(|(_, v)| v.into_owned());
    Ok((project.to_string(), name, location))
}

fn query(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == "query")
        .map(|(_, v)| v.into_owned())
}

async fn token(extra_env: &HashMap<String, String>) -> Result<String> {
    if let Some(token) = env_lookup(
        extra_env,
        &[
            "GOOGLE_OAUTH_ACCESS_TOKEN",
            "CLOUDSDK_AUTH_ACCESS_TOKEN",
            "LADE_GCP_TOKEN",
        ],
    ) {
        return Ok(token);
    }
    let provider = gcp_auth::provider()
        .await
        .map_err(|e| anyhow!("GCP Secret Manager error: {e}. See {DOCS}."))?;
    let token = gcp_auth::TokenProvider::token(
        &*provider,
        &["https://www.googleapis.com/auth/cloud-platform"],
    )
    .await
    .map_err(|e| anyhow!("GCP Secret Manager error: {e}. See {DOCS}."))?;
    Ok(token.as_str().to_string())
}

#[async_trait]
impl Provider for GcpSm {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value.clone(), "gcpsm")?;
        let url = Url::parse(&value)?;
        project_and_name(&url)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "GCP Secret Manager"
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn transport(&self) -> Transport {
        Transport::Sdk
    }

    fn batch_unit(&self) -> &'static str {
        "(project, name, location)"
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
        let api: Arc<dyn GcpSmApi> = if let Some(api) = &self.api {
            Arc::clone(api)
        } else {
            Arc::new(RestGcpSm {
                token: token(extra_env).await?,
                client: reqwest::Client::new(),
            })
        };
        let mut by_secret: HashMap<
            (String, String, Option<String>),
            Vec<(String, Option<String>)>,
        > = HashMap::new();
        for (url, raw) in &self.urls {
            let (project, name, location) = project_and_name(url)?;
            by_secret
                .entry((project, name, location))
                .or_default()
                .push((raw.clone(), query(url)));
        }
        let fetches = by_secret
            .into_iter()
            .map(|((project, name, location), uris)| {
                let api = Arc::clone(&api);
                async move {
                    let value = api.access(&project, &name, location.as_deref()).await?;
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
