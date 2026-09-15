use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::FxHashMap;
use url::Url;

use crate::Hydration;

use super::{Provider, Warnings, add_url, json_query_string, require_cli_ok, run_cli, search_cli};

const DOCS: &str = "https://cloud.google.com/sdk/gcloud/reference/secrets/versions/access";

#[derive(Default)]
pub struct GcpSm {
    urls: FxHashMap<Url, String>,
}

impl GcpSm {
    pub fn new() -> Self {
        Default::default()
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

    fn search(&self, extra_env: &HashMap<String, String>) -> Result<Vec<String>> {
        let output = search_cli(
            "gcloud",
            &["secrets", "list", "--format=value(name)"],
            extra_env,
        )?;
        if !output.status.success() {
            return Err(anyhow!("gcloud login required"));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| line.split('/').next_back().unwrap_or(line).to_string())
            .collect())
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
        let extra_env = Arc::new(extra_env.clone());
        let name = self.name();
        let install_url = self.install_url();
        let mut by_secret: HashMap<
            (String, String, Option<String>),
            Vec<(String, Option<String>)>,
        > = HashMap::new();
        for (url, raw) in &self.urls {
            let (project, secret, location) = project_and_name(url)?;
            by_secret
                .entry((project, secret, location))
                .or_default()
                .push((raw.clone(), query(url)));
        }
        let fetches = by_secret.into_iter().map(|(key, uris)| {
            let extra_env = Arc::clone(&extra_env);
            async move {
                let (project, secret, location) = key;
                let mut args = vec![
                    "gcloud".to_string(),
                    "secrets".to_string(),
                    "versions".to_string(),
                    "access".to_string(),
                    "latest".to_string(),
                    format!("--secret={secret}"),
                    format!("--project={project}"),
                ];
                if let Some(location) = location {
                    args.push(format!("--location={location}"));
                }
                let cmd: Vec<&str> = args.iter().map(String::as_str).collect();
                let output = run_cli(&cmd, &extra_env, name, install_url, None).await?;
                require_cli_ok(&output, name)?;
                let value = String::from_utf8_lossy(&output.stdout).to_string();
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
