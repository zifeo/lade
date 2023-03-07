use std::collections::HashMap;

use crate::Hydration;
use anyhow::{anyhow, bail, Ok, Result};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::{debug, info};
use url::Url;
use uuid::Uuid;

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
    async fn resolve(&self) -> Result<Hydration> {
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|u| u.host().expect("Missing host"))
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
                                    let secret = format!("{}.json", Uuid::new_v4());
                                    let cmd = [
                                        "doppler",
                                        "--api-host",
                                        &format!("https://{}", host),
                                        "run",
                                        "--project",
                                        project,
                                        "--config",
                                        env,
                                        "--mount",
                                        &secret,
                                        "--mount-format",
                                        "json",
                                        "--",
                                        "cat",
                                        &secret,
                                    ];
                                    info!("{}", cmd.join(" "));

                                    let child = Command::new(cmd[0])
                                        .args(&cmd[1..])
                                        .stdout(Stdio::piped())
                                        .stderr(Stdio::piped())
                                        .output()
                                        .await
                                        .expect("error running doppler");

                                    let loaded = serde_json::from_slice::<Hydration>(&child.stdout)
                                        .map_err(|_| {
                                            let err = String::from_utf8_lossy(&child.stderr);
                                            anyhow!("Doppler error: {err}",)
                                        })?;

                                    let hydration = vars
                                        .into_iter()
                                        .map(|(key, var)| {
                                            (
                                                var,
                                                loaded
                                                    .get(key)
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
