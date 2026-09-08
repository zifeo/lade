use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::debug;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use std::{collections::HashMap, fs::File, io::Write, path::Path, sync::Arc};
use tempfile::tempdir;
use url::Url;

use crate::Hydration;

use super::{Provider, Transport, Warnings, add_url, host_with_port, run_cli};

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
struct InfisicalExport {
    key: String,
    value: String,
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
        String::new()
    };
    Ok((path, variable))
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
        "https://infisical.com/docs/cli/overview"
    }

    fn transport(&self) -> Transport {
        Transport::Cli
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
        let extra_env = Arc::new(extra_env.clone());
        let name = self.name();
        let install_url = self.install_url();
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
                                let extra_env = Arc::clone(&extra_env);
                                let project = project.clone();
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

                                    let temp_dir = tempdir()?;
                                    let config =
                                        HashMap::from([("workspaceId", project.as_str()), ("defaultEnvironment", "")]);
                                    let config_path = temp_dir.path().join(".infisical.json");
                                    let mut file = File::create(config_path)?;
                                    write!(file, "{}", serde_json::to_string(&config)?)?;
                                    drop(file);

                                    let temp_dir_path = Arc::new(temp_dir.path().to_path_buf());
                                    let path_futures = path_groups.into_iter().map(|(path, variables)| {
                                        let host = host.clone();
                                        let extra_env = Arc::clone(&extra_env);
                                        let temp_dir_path = Arc::clone(&temp_dir_path);
                                        let project = project.clone();
                                        let env = env.clone();
                                        async move {
                                            let domain = format!("https://{host}/api");
                                            let path_arg = if path.is_empty() {
                                                "/".to_string()
                                            } else {
                                                path.clone()
                                            };
                                            let cmd = [
                                                "infisical",
                                                "--domain",
                                                &domain,
                                                "export",
                                                "--path",
                                                &path_arg,
                                                "--env",
                                                &env,
                                                "--projectId",
                                                &project,
                                                "--format",
                                                "json",
                                            ];
                                            let child = run_cli(
                                                &cmd,
                                                &extra_env,
                                                name,
                                                install_url,
                                                Some(&temp_dir_path),
                                            )
                                            .await?;
                                            let loaded =
                                                serde_json::from_slice::<Vec<InfisicalExport>>(&child.stdout)
                                                    .map_err(|err| {
                                                        let stderr = String::from_utf8_lossy(&child.stderr);
                                                        if stderr.contains("login expired") {
                                                            anyhow!(
                                                                "Login expired for Infisical instance {host}: {stderr}"
                                                            )
                                                        } else if stderr.contains("unable to validate environment") {
                                                            anyhow!(
                                                                "Workspace seems not accessible from logged account on {host}: {stderr}"
                                                            )
                                                        } else {
                                                            anyhow!("Infisical error: {err} (stderr: {stderr})")
                                                        }
                                                    })?
                                                    .into_iter()
                                                    .map(|e| (e.key, e.value))
                                                    .collect::<Vec<_>>();

                                            let mut missing_vars = Vec::new();
                                            let mut partial = Hydration::default();
                                            for (var_name, original_url) in variables {
                                                if let Some((_, value)) =
                                                    loaded.iter().find(|(key, _)| key == &var_name)
                                                {
                                                    partial.insert(original_url, value.clone());
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
                                    });

                                    let hydration: Hydration = try_join_all(path_futures)
                                        .await?
                                        .into_iter()
                                        .flatten()
                                        .collect();
                                    temp_dir.close()?;
                                    debug!("hydration: {:?}", hydration);
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
