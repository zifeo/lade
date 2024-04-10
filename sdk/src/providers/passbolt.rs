use std::{collections::HashMap, path::Path};

use anyhow::{anyhow, bail, Result};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::future::try_join_all;

use itertools::Itertools;
use log::debug;
use url::Url;

use crate::{providers::envs, Hydration};

use super::Provider;

#[derive(Default)]
pub struct Passbolt {
    urls: HashMap<Url, String>,
}

impl Passbolt {
    pub fn new() -> Self {
        Default::default()
    }
}

#[async_trait]
impl Provider for Passbolt {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "passbolt" => {
                self.urls.insert(url, value);
                Ok(())
            }
            _ => bail!("Not a passbolt scheme"),
        }
    }

    async fn resolve(&self, _: &Path) -> Result<Hydration> {
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|(url, _)| url.host().expect("Missing host"))
            .into_iter()
            .flat_map(|(host, group)| {
                group
                    .into_iter()
                    .into_group_map_by(|(url, _)| url.path().split('/').nth(1).expect("Missing resource id"))
                    .into_iter()
                    .map(|(resource_id, group)| {
                        let host = host.clone();
                        async move {
                            let cmd = [
                                "passbolt",
                                "get",
                                "resource",
                                // #[cfg(debug_assertions)]
                                // &format!("--serverAddress=http://{}", host),
                                // #[cfg(not(debug_assertions))]
                                &format!("--serverAddress=https://{}", host),
                                &format!("--id={}", resource_id),
                                "--json"
                            ];
                            debug!("Lade run: {}", cmd.join(" "));

                            let child = match Command::new(cmd[0])
                                .args(&cmd[1..])
                                .envs(envs())
                                .stdout(Stdio::piped())
                                .stderr(Stdio::piped())
                                .output()
                                .await {
                                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                        bail!("Passbolt CLI not found. Make sure the binary is in your PATH or install it from https://github.com/passbolt/go-passbolt-cli.")
                                    },
                                    Err(e) => {
                                        bail!("Passbolt error: {e}")
                                    },
                                    Ok(child) => child,
                                };

                            let loaded =
                                serde_json::from_slice::<HashMap<String, String>>(&child.stdout)
                                    .map_err(|err| {
                                        let stderr = String::from_utf8_lossy(&child.stderr);
                                        anyhow!("Passbolt error: {err} (stderr: {stderr})")
                                    })?;

                            let hydration = group
                                .into_iter()
                                .map(|(url, value)| {
                                    let var = url.path()
                                                    .split('/')
                                                    .nth(2)
                                                    .expect("Missing field");
                                    (
                                        value.clone(),
                                        loaded
                                            .get(var,
                                            )
                                            .unwrap_or_else(|| panic!(
                                                "Variable not found in Passbolt: {}",
                                                resource_id
                                            ))
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
            .collect::<Vec<_>>();

        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }
}
