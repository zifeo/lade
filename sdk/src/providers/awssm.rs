use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::FxHashMap;
use url::Url;

use crate::Hydration;

use super::{Provider, Transport, Warnings, add_url, json_query_string};

const DOCS: &str =
    "https://docs.aws.amazon.com/secretsmanager/latest/apireference/API_BatchGetSecretValue.html";
const BATCH: usize = 20;

#[async_trait]
pub(crate) trait AwsSmApi: Send + Sync {
    async fn batch_get(&self, region: &str, ids: &[String]) -> Result<HashMap<String, String>>;
    async fn get(
        &self,
        region: &str,
        id: &str,
        version_id: Option<&str>,
        version_stage: Option<&str>,
    ) -> Result<String>;
}

#[derive(Default)]
struct SdkAwsSm {
    clients: tokio::sync::Mutex<HashMap<String, aws_sdk_secretsmanager::Client>>,
}

impl SdkAwsSm {
    async fn client(&self, region: &str) -> aws_sdk_secretsmanager::Client {
        let mut cache = self.clients.lock().await;
        if let Some(client) = cache.get(region) {
            return client.clone();
        }
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .load()
            .await;
        let client = aws_sdk_secretsmanager::Client::new(&config);
        cache.insert(region.to_string(), client.clone());
        client
    }
}

#[async_trait]
impl AwsSmApi for SdkAwsSm {
    async fn batch_get(&self, region: &str, ids: &[String]) -> Result<HashMap<String, String>> {
        let client = self.client(region).await;
        let response = client
            .batch_get_secret_value()
            .set_secret_id_list(Some(ids.to_vec()))
            .send()
            .await
            .map_err(|e| anyhow!("AWS Secrets Manager error: {e}. See {DOCS}."))?;
        let mut out = HashMap::new();
        for secret in response.secret_values() {
            let value = secret.secret_string().ok_or_else(|| {
                anyhow!(
                    "AWS Secrets Manager error: secret has no string value (SecretBinary is not supported). See {DOCS}."
                )
            })?;
            if let Some(name) = secret.name() {
                out.insert(name.to_string(), value.to_string());
            }
            if let Some(arn) = secret.arn() {
                out.insert(arn.to_string(), value.to_string());
            }
        }
        if !response.errors().is_empty() {
            let msgs: Vec<String> = response
                .errors()
                .iter()
                .map(|e| {
                    format!(
                        "{}: {}",
                        e.secret_id().unwrap_or("unknown"),
                        e.message().unwrap_or("unknown error")
                    )
                })
                .collect();
            bail!(
                "AWS Secrets Manager error: {}. See {DOCS}.",
                msgs.join("; ")
            );
        }
        for id in ids {
            if !out.contains_key(id) {
                bail!("AWS Secrets Manager error: secret '{id}' not found. See {DOCS}.");
            }
        }
        Ok(out)
    }

    async fn get(
        &self,
        region: &str,
        id: &str,
        version_id: Option<&str>,
        version_stage: Option<&str>,
    ) -> Result<String> {
        let client = self.client(region).await;
        let mut req = client.get_secret_value().secret_id(id);
        if let Some(version_id) = version_id {
            req = req.version_id(version_id);
        }
        if let Some(version_stage) = version_stage {
            req = req.version_stage(version_stage);
        }
        let response = req
            .send()
            .await
            .map_err(|e| anyhow!("AWS Secrets Manager error: {e}. See {DOCS}."))?;
        response.secret_string().map(|s| s.to_string()).ok_or_else(|| {
            anyhow!(
                "AWS Secrets Manager error: '{id}' has no string value (SecretBinary is not supported). See {DOCS}."
            )
        })
    }
}

#[derive(Default)]
pub struct AwsSm {
    urls: FxHashMap<Url, String>,
    api: Option<Arc<dyn AwsSmApi>>,
}

impl AwsSm {
    pub fn new() -> Self {
        Default::default()
    }

    #[cfg(test)]
    pub(crate) fn with_api(api: Arc<dyn AwsSmApi>) -> Self {
        Self {
            urls: FxHashMap::default(),
            api: Some(api),
        }
    }
}

fn secret_id(url: &Url) -> Result<String> {
    let path = url.path().trim_start_matches('/');
    if path.is_empty() {
        bail!("AWS Secrets Manager URI missing secret name");
    }
    urlencoding::decode(path)
        .map(|s| s.into_owned())
        .map_err(|e| anyhow!("AWS Secrets Manager error: {e}"))
}

fn region(url: &Url) -> Result<String> {
    match url.host_str() {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => bail!("AWS Secrets Manager URI missing region"),
    }
}

fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

#[async_trait]
impl Provider for AwsSm {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value.clone(), "awssm")?;
        let url = Url::parse(&value)?;
        region(&url)?;
        secret_id(&url)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "AWS Secrets Manager"
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn transport(&self) -> Transport {
        Transport::Sdk
    }

    fn batch_unit(&self) -> &'static str {
        "(region, name)"
    }

    fn has_work(&self) -> bool {
        !self.urls.is_empty()
    }

    async fn resolve(
        &self,
        _: &Path,
        _: &HashMap<String, String>,
        _: &Warnings,
    ) -> Result<Hydration> {
        let api: Arc<dyn AwsSmApi> = self
            .api
            .clone()
            .unwrap_or_else(|| Arc::new(SdkAwsSm::default()));
        let mut by_region: HashMap<
            String,
            Vec<(
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            )>,
        > = HashMap::new();
        for (url, raw) in &self.urls {
            by_region.entry(region(url)?).or_default().push((
                secret_id(url)?,
                raw.clone(),
                query_param(url, "query"),
                query_param(url, "version"),
                query_param(url, "version_stage"),
            ));
        }
        let fetches = by_region.into_iter().map(|(region, items)| {
            let api = Arc::clone(&api);
            async move {
                let mut unique = Vec::new();
                for (id, _, _, version, stage) in &items {
                    if version.is_none() && stage.is_none() && !unique.contains(id) {
                        unique.push(id.clone());
                    }
                }
                let mut loaded = HashMap::new();
                for chunk in unique.chunks(BATCH) {
                    loaded.extend(api.batch_get(&region, chunk).await?);
                }
                let mut hydration = Hydration::default();
                for (id, raw, q, version, stage) in items {
                    let value = if version.is_some() || stage.is_some() {
                        api.get(&region, &id, version.as_deref(), stage.as_deref())
                            .await?
                    } else {
                        loaded.get(&id).cloned().ok_or_else(|| {
                            anyhow!(
                                "AWS Secrets Manager error: secret '{id}' not found. See {DOCS}."
                            )
                        })?
                    };
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
