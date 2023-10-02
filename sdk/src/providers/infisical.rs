use anyhow::{anyhow, bail, Result};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::debug;
use serde::Deserialize;
use std::{collections::HashMap, fs::File, io::Write, path::Path};
use tempfile::tempdir;
use url::Url;

use crate::{providers::envs, Hydration};

use super::Provider;

#[derive(Default)]
pub struct Infisical {
    urls: HashMap<Url, String>,
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

#[async_trait]
impl Provider for Infisical {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "infisical" => {
                self.urls.insert(url, value);
                Ok(())
            }
            _ => bail!("Not an infisical scheme"),
        }
    }
    async fn resolve(&self, _: &Path) -> Result<Hydration> {
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|(url, _)| {
                let port = match url.port() {
                    Some(port) => format!(":{}", port),
                    None => "".to_string(),
                };
                format!("{}{}", url.host().expect("Missing host"), port)
            })
            .into_iter()
            .flat_map(|(host, group)| {
                group
                    .into_iter()
                    .into_group_map_by(|(url, _)| url.path().split('/').nth(1).expect("Missing project"))
                    .into_iter()
                    .flat_map(|(project, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|(url, _)| {
                                (url.path().split('/').nth(2)).expect("Missing env")
                            })
                            .into_iter()
                            .map(|(env, group)| {
                                let vars = group
                                    .iter()
                                    .map(|(url, value)| {
                                        (
                                            url.path().split('/').nth(3).expect("Missing variable"),
                                            (*value).clone(),
                                        )
                                    })
                                    .collect::<HashMap<_, _>>();

                                let host = host.clone();
                                async move {
                                    let cmd = [
                                        "infisical",
                                        "--domain",
                                        &format!("https://{}/api", host),
                                        "export",
                                        "--env",
                                        env,
                                        "--format",
                                        "json",
                                    ];
                                    debug!("Lade run: {}", cmd.join(" "));

                                    let temp_dir = tempdir()?;
                                    let config = HashMap::from([
                                        ("workspaceId", project),
                                        ("defaultEnvironment", ""),
                                    ]);
                                    let path = temp_dir.path().join(".infisical.json");
                                    let mut file = File::create(path)?;
                                    write!(file, "{}", serde_json::to_string(&config)?)?;
                                    drop(file);

                                    let child = match Command::new(cmd[0])
                                        .args(&cmd[1..])
                                        .current_dir(temp_dir.path())
                                        .envs(envs())
                                        .stdout(Stdio::piped())
                                        .stderr(Stdio::piped())
                                        .output()
                                        .await
                                         {
                                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                                bail!("Infisical CLI not found. Make sure the binary is in your PATH or install it from https://infisical.com/docs/cli/overview.")
                                            },
                                            Err(e) => {
                                                bail!("Infisical error: {e}")
                                            },
                                            Ok(child) => child,
                                        };

                                    temp_dir.close()?;

                                    let loaded = serde_json::from_slice::<Vec<InfisicalExport>>(
                                        &child.stdout,
                                    )
                                    .map_err(|err| {
                                        let stderr = String::from_utf8_lossy(&child.stderr);
                                        if stderr.contains("login expired") {
                                            anyhow!(
                                                "Login expired for Infisical instance {host}: {stderr}",
                                            )
                                        } else if stderr.contains("unable to validate environment") {
                                            anyhow!(
                                                "Workspace seems not accessible from logged account on {host}: {stderr}",
                                            )
                                        } else {
                                            anyhow!("Infisical error: {err} (stderr: {stderr})")
                                        }
                                    })?
                                    .into_iter()
                                    .map(|e| (e.key, e.value))
                                    .collect::<Hydration>();

                                    let hydration = vars
                                        .into_iter()
                                        .map(|(key, value)| {
                                            (
                                                value,
                                                loaded
                                                    .get(key)
                                                    .unwrap_or_else(|| panic!("Variable not found in Infisical: {}", key))
                                                    .clone(),
                                            )
                                        })
                                        .collect::<Hydration>();

                                    debug!("hydration: {:?}", hydration);
                                    Ok(hydration)
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
