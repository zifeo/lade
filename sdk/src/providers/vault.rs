use anyhow::{Result, anyhow, bail};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::debug;
use serde::Deserialize;
use std::{collections::HashMap, path::Path};
use url::Url;

use crate::{Hydration, providers::envs};

use super::Provider;

#[derive(Default)]
pub struct Vault {
    urls: HashMap<Url, String>,
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
struct VaultExport {
    data: VaultGetKVData,
}

#[async_trait]
impl Provider for Vault {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "vault" => {
                self.urls.insert(url, value);
                Ok(())
            }
            _ => bail!("Not an vault scheme"),
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
                    .flat_map(|(mount, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|(url, _)| {
                                (url.path().split('/').nth(2)).expect("Missing env")
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
                                        &urlencoding::decode(key).expect("Invalid URL key decoding"),
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
                                                bail!("Vault CLI not found. Make sure the binary is in your PATH or install it from https://developer.hashicorp.com/vault/docs/commands.")
                                            },
                                            Err(e) => {
                                                bail!("Vault error: {e}")
                                            },
                                            Ok(child) => child,
                                        };

                                    let loaded =
                                        serde_json::from_slice::<VaultExport>(&child.stdout)
                                            .map_err(|err| {
                                                let stderr = String::from_utf8_lossy(&child.stderr);
                                                anyhow!("Vault error: {err} (stderr: {stderr})")
                                            })?
                                            .data
                                            .data;

                                    let hydration = group
                                        .into_iter()
                                        .map(|(url, value)| {
                                            let var = url.path()
                                                            .split('/')
                                                            .nth(3)
                                                            .expect("Missing variable");
                                            (
                                                value.clone(),
                                                loaded
                                                    .get(
                                                        urlencoding::decode(var).expect("Invalid URL field decoding").as_ref(),
                                                    )
                                                    .unwrap_or_else(|| panic!(
                                                        "Variable not found in Vault: {}",
                                                        key
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
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }
}
