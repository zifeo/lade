use std::{collections::HashMap, path::Path};

use anyhow::{anyhow, bail, Result};
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
    async fn resolve(&self, _: &Path) -> Result<Hydration> {
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

                    let json = &vars
                        .iter()
                        .map(|(k, v)| (k, v.replace(&format!("{host}/"), "").replace("%20", " ")))
                        .collect::<HashMap<_, _>>();
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
                    stdin
                        .write_all(serde_json::to_string(&json)?.as_bytes())
                        .await?;
                    drop(stdin);

                    let child = match process.output().await  {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            bail!("1Password CLI not found. Make sure the binary is in your PATH or install it from https://1password.com/downloads/command-line/.")
                        },
                        Err(e) => {
                            bail!("1Password error: {e}")
                        },
                        Ok(child) => child,
                    };

                    let loaded =
                        serde_json::from_slice::<Hydration>(&child.stdout).map_err(|err| {
                            let stderr = String::from_utf8_lossy(&child.stderr);
                            anyhow!("1Password error: {err} (stderr: {stderr})",)
                        })?;

                    let hydration = vars
                        .iter()
                        .map(|(key, var)| {
                            let value = match (loaded.get(key), json.get(key)) {
                                (Some(loaded), Some(original)) if loaded == original => None,
                                (Some(loaded), _) => Some(loaded),
                                _ => None,
                            };

                            (
                                var.clone(),
                                value
                                    .unwrap_or_else(|| panic!("Variable not found in 1Password: {}", key))
                                    .clone(),
                            )
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
