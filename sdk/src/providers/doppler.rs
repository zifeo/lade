use std::{collections::HashMap, path::Path};

use crate::Hydration;
use anyhow::{anyhow, bail, Result};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::debug;
use serde::Deserialize;
use url::Url;

use super::Provider;

#[derive(Default)]
pub struct Doppler {
    urls: Vec<Url>,
}

impl Doppler {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Deserialize)]
struct DopplerExport {
    computed: String,
}

#[async_trait]
impl Provider for Doppler {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "doppler" => {
                self.urls.push(url);
                Ok(())
            }
            _ => bail!("Not a doppler scheme"),
        }
    }
    async fn resolve(&self, _: &Path) -> Result<Hydration> {
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|u| {
                let port = match u.port() {
                    Some(port) => format!(":{}", port),
                    None => "".to_string(),
                };
                format!("{}{}", u.host().expect("Missing host"), port)
            })
            .into_iter()
            .flat_map(|(host, group)| {
                group
                    .into_iter()
                    .into_group_map_by(|u| u.path().split('/').nth(1).expect("Missing project"))
                    .into_iter()
                    .flat_map(|(project, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|u| u.path().split('/').nth(2).expect("Missing env"))
                            .into_iter()
                            .map(|(env, group)| {
                                let vars = group
                                    .iter()
                                    .map(|u| {
                                        (
                                            u.path().split('/').nth(3).expect("Missing variable"),
                                            u.to_string(),
                                        )
                                    })
                                    .collect::<HashMap<_, _>>();

                                let host = host.clone();
                                async move {
                                    let cmd = [
                                        "doppler",
                                        "--api-host",
                                        &format!("https://{}", host),
                                        "secrets",
                                        "--project",
                                        project,
                                        "--config",
                                        env,
                                        "--json",
                                    ];
                                    debug!("Lade run: {}", cmd.join(" "));

                                    let child = match Command::new(cmd[0])
                                        .args(&cmd[1..])
                                        .stdout(Stdio::piped())
                                        .stderr(Stdio::piped())
                                        .output()
                                        .await {
                                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                                bail!("Doppler CLI not found. Make sure the binary is in your PATH or install it from https://docs.doppler.com/docs/install-cli.")
                                            },
                                            Err(e) => {
                                                bail!("Doppler error: {e}")
                                            },
                                            Ok(child) => child,
                                        };

                                    let loaded = serde_json::from_slice::<
                                        HashMap<String, DopplerExport>,
                                    >(
                                        &child.stdout
                                    )
                                    .map_err(|err| {
                                        let stderr = String::from_utf8_lossy(&child.stderr);
                                        anyhow!("Doppler error: {err} (stderr: {stderr})",)
                                    })?;

                                    let hydration = vars
                                        .into_iter()
                                        .map(|(key, var)| {
                                            (
                                                var,
                                                loaded
                                                    .get(key)
                                                    .unwrap_or_else(|| panic!(
                                                        "Variable not found in Doppler: {}",
                                                        key
                                                    ))
                                                    .computed
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
