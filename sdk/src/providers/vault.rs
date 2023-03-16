use anyhow::{anyhow, bail, Ok, Result};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::{debug, info};
use serde::Deserialize;
use std::collections::HashMap;
use url::Url;

use crate::Hydration;

use super::Provider;

#[derive(Default)]
pub struct Vault {
    urls: Vec<Url>,
}

impl Vault {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Deserialize)]
struct VaultGetKVData {
    data: HashMap<String, String>,
}

#[derive(Deserialize)]
struct VaultGetKV {
    data: VaultGetKVData,
}

#[async_trait]
impl Provider for Vault {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "vault" => {
                self.urls.push(url);
                Ok(())
            }
            _ => bail!("Not an vault scheme"),
        }
    }
    async fn resolve(&self) -> Result<Hydration> {
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
                    .flat_map(|(mount, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|u| {
                                (u.path().split('/').nth(2)).expect("Missing env")
                            })
                            .into_iter()
                            .map(|(key, group)| {
                                let host = host.clone();
                                async move {
                                    let cmd = [
                                        "vault",
                                        "kv",
                                        "get",
                                        #[cfg(debug_assertions)]
                                        &format!("-address=http://{}", host),
                                        #[cfg(not(debug_assertions))]
                                        &format!("-address=https://{}", host),
                                        &format!("-mount={}", mount),
                                        "-format=json",
                                        key,
                                    ];
                                    info!("{}", cmd.join(" "));

                                    let child = Command::new(cmd[0])
                                        .args(&cmd[1..])
                                        .stdout(Stdio::piped())
                                        .stderr(Stdio::piped())
                                        .output()
                                        .await
                                        .expect("error running Vault");

                                    let loaded =
                                        serde_json::from_slice::<VaultGetKV>(&child.stdout)
                                            .map_err(|_| {
                                                let err = String::from_utf8_lossy(&child.stderr);
                                                anyhow!("Vault error: {err}")
                                            })?
                                            .data
                                            .data;

                                    let hydration = group
                                        .into_iter()
                                        .map(|u| {
                                            (
                                                u.to_string(),
                                                loaded
                                                    .get(
                                                        u.path()
                                                            .split('/')
                                                            .nth(3)
                                                            .expect("Missing variable"),
                                                    )
                                                    .expect("Variable not found")
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
