use std::{collections::HashMap, path::Path};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use rustc_hash::FxHashMap;
use url::Url;

use crate::Hydration;

use super::{Provider, Warnings, add_url, fetch_grouped, query_param, search_cli_ok};

const DOCS: &str =
    "https://docs.aws.amazon.com/cli/latest/reference/secretsmanager/get-secret-value.html";

#[derive(Default)]
pub struct AwsSm {
    urls: FxHashMap<Url, String>,
}

impl AwsSm {
    pub fn new() -> Self {
        Default::default()
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

fn secret_string(raw: &str, id: &str) -> Result<String> {
    let json: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| anyhow!("AWS Secrets Manager error: {e}. See {DOCS}."))?;
    json.get("SecretString")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            anyhow!(
                "AWS Secrets Manager error: '{id}' has no string value (SecretBinary is not supported). See {DOCS}."
            )
        })
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

    fn search(&self, extra_env: &HashMap<String, String>) -> Result<Vec<String>> {
        let output = search_cli_ok(
            "aws",
            &[
                "secretsmanager",
                "list-secrets",
                "--query",
                "SecretList[].Name",
                "--output",
                "text",
            ],
            extra_env,
            "aws login required",
        )?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .map(str::to_string)
            .collect())
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
        extra_env: &HashMap<String, String>,
        _: &Warnings,
    ) -> Result<Hydration> {
        let mut by_secret: HashMap<
            (String, String, Option<String>, Option<String>),
            Vec<(String, Option<String>)>,
        > = HashMap::new();
        for (url, raw) in &self.urls {
            by_secret
                .entry((
                    region(url)?,
                    secret_id(url)?,
                    query_param(url, "version"),
                    query_param(url, "version_stage"),
                ))
                .or_default()
                .push((raw.clone(), query_param(url, "query")));
        }
        fetch_grouped(
            by_secret,
            extra_env,
            self.name(),
            self.install_url(),
            |(region, id, version, stage)| {
                let mut args = vec![
                    "aws".to_string(),
                    "secretsmanager".to_string(),
                    "get-secret-value".to_string(),
                    "--secret-id".to_string(),
                    id.clone(),
                    "--region".to_string(),
                    region.clone(),
                    "--output".to_string(),
                    "json".to_string(),
                ];
                if let Some(version) = version {
                    args.push("--version-id".to_string());
                    args.push(version.clone());
                }
                if let Some(stage) = stage {
                    args.push("--version-stage".to_string());
                    args.push(stage.clone());
                }
                args
            },
            |stdout, (_, id, _, _)| secret_string(stdout, id),
        )
        .await
    }

    fn compose_add_uri(&self, picked: &str, extras: &HashMap<String, String>) -> Option<String> {
        let region = extras
            .get("region")
            .map(String::as_str)
            .unwrap_or("us-east-1");
        Some(format!("awssm://{region}/{picked}"))
    }
}

pub(super) const ADD_FIELDS: &[super::add::AddField] = &[super::add::AddField {
    key: "region",
    prompt: "Region (us-east-1): ",
    default: Some("us-east-1"),
}];

#[cfg(test)]
mod tests;
