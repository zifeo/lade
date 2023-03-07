use std::collections::HashMap;

use anyhow::{anyhow, bail, Ok, Result};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::{future::try_join_all, AsyncWriteExt};

use itertools::Itertools;
use log::{debug, info};
use url::Url;

use crate::Hydration;

use super::Provider;

#[derive(Default)]
pub struct OnePassword {
    urls: Vec<Url>,
}

impl OnePassword {
    pub fn new() -> Self {
        Default::default()
    }
}

#[async_trait]
impl Provider for OnePassword {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "op" => {
                self.urls.push(url);
                Ok(())
            }
            _ => bail!("Not a onepassword scheme"),
        }
    }
    async fn resolve(&self) -> Result<Hydration> {
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|u| u.host().expect("Missing host"))
            .into_iter()
            .map(|(host, group)| {
                let vars = group
                    .iter()
                    .enumerate()
                    .map(|(idx, u)| (idx.to_string(), u.to_string()))
                    .collect::<HashMap<_, _>>();

                let host = host.clone();
                async move {
                    if vars.is_empty() {
                        return Ok(HashMap::new());
                    }

                    let json = serde_json::to_string(
                        &vars
                            .iter()
                            .map(|(k, v)| {
                                (k, v.replace(&format!("{host}/"), "").replace("%20", " "))
                            })
                            .collect::<HashMap<_, _>>(),
                    )?;
                    let cmd = &["op", "inject", "--account", &host.to_string()];
                    info!("{}", cmd.join(" "));

                    let mut process = Command::new(cmd[0])
                        .args(&cmd[1..])
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .stdin(Stdio::piped())
                        .spawn()?;

                    debug!("stdin: {:?}", json);

                    let mut stdin = process.stdin.take().expect("Failed to open stdin");
                    stdin.write_all(json.as_bytes()).await?;
                    drop(stdin);

                    let child = process.output().await?;

                    let loaded =
                        serde_json::from_slice::<Hydration>(&child.stdout).map_err(|_| {
                            let err = String::from_utf8_lossy(&child.stderr);
                            anyhow!("1Password error: {err}",)
                        })?;

                    let hydration = vars
                        .into_iter()
                        .map(|(key, var)| {
                            (var, loaded.get(&key).expect("Variable not found").clone())
                        })
                        .collect::<Hydration>();

                    debug!("hydration: {:?}", hydration);
                    Ok(hydration)
                }
            })
            .collect::<Vec<_>>();

        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }
}
