use std::{collections::HashMap, path::Path};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use rustc_hash::FxHashMap;
use url::Url;

use crate::Hydration;

use super::{Provider, Warnings, add_url, fetch_grouped, query_param, search_cli_ok};

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

#[async_trait]
impl Provider for GcpSm {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value.clone(), "gcpsm")?;
        let url = Url::parse(&value)?;
        project_and_name(&url)?;
        Ok(())
    }

    fn timeout(&self) -> std::time::Duration {
        super::PROVIDER_TIMEOUT
    }

    fn name(&self) -> &'static str {
        "GCP Secret Manager"
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn search(&self, extra_env: &HashMap<String, String>) -> Result<Vec<String>> {
        let output = search_cli_ok(
            "gcloud",
            &["secrets", "list", "--format=value(name)"],
            extra_env,
            self.timeout(),
            "gcloud login required",
        )?;
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
        let mut by_secret: HashMap<
            (String, String, Option<String>),
            Vec<(String, Option<String>)>,
        > = HashMap::new();
        for (url, raw) in &self.urls {
            let (project, secret, location) = project_and_name(url)?;
            by_secret
                .entry((project, secret, location))
                .or_default()
                .push((raw.clone(), query_param(url, "query")));
        }
        fetch_grouped(
            by_secret,
            extra_env,
            self.name(),
            self.install_url(),
            |(project, secret, location)| {
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
                args
            },
            |stdout, _| Ok(stdout.to_string()),
        )
        .await
    }

    fn compose_add_uri(&self, picked: &str, extras: &HashMap<String, String>) -> Option<String> {
        let project = extras
            .get("project")
            .map(String::as_str)
            .unwrap_or("project");
        Some(format!("gcpsm://{project}/{picked}"))
    }
}

pub(super) const ADD_FIELDS: &[super::add::AddField] = &[super::add::AddField {
    key: "project",
    prompt: "Project: ",
    default: None,
}];

#[cfg(test)]
mod tests;
