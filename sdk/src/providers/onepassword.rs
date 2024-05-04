use std::{collections::HashMap, path::Path};

use anyhow::{bail, Result};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::{future::try_join_all, AsyncWriteExt};

use itertools::Itertools;
use log::debug;
use url::Url;

use crate::{providers::envs, Hydration};

use super::Provider;

#[derive(Default)]
pub struct OnePassword {
    urls: HashMap<Url, String>,
}

impl OnePassword {
    pub fn new() -> Self {
        Default::default()
    }
}

static SEP: &str = "'Km5Ge8AbNc+QSBauOIN0jg'";

#[async_trait]
impl Provider for OnePassword {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "op" => {
                self.urls.insert(url, value);
                Ok(())
            }
            _ => bail!("Not a onepassword scheme"),
        }
    }
    async fn resolve(&self, _: &Path) -> Result<Hydration> {
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|(url, _)| url.host().expect("Missing host"))
            .into_iter()
            .map(|(host, group)| {
                let vars = group
                    .into_iter()
                    .enumerate()
                    .map(|(idx, (_, value))| (idx.to_string(), value.clone()))
                    .collect::<HashMap<_, _>>();

                let host = host.clone();
                async move {
                    if vars.is_empty() {
                        return Ok(HashMap::new());
                    }

                    let input = &vars.values()
                        .map(|v| v.replace(&format!("{host}/"), "").replace("%20", " "))
                        .join(SEP);
                    let cmd = &["op", "inject", "--account", &host.to_string()];
                    debug!("Lade run: {}", cmd.join(" "));

                    let mut process = Command::new(cmd[0])
                        .args(&cmd[1..])
                        .envs(envs())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .stdin(Stdio::piped())
                        .spawn()?;

                    debug!("stdin: {:?}", input);

                    let mut stdin = process.stdin.take().expect("Failed to open stdin");
                    stdin
                        .write_all(input.as_bytes())
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

                    let output = String::from_utf8_lossy(&child.stdout).trim().replace('\n', "\\n");
                    debug!("stdout: {:?}", output);
                    let loaded = output.split(SEP).collect::<Vec<_>>();

                    let hydration = vars
                        .iter().zip_eq(loaded)
                        .map(|((_, key), value)| {
                            (
                                key.clone(),
                                value.to_string(),
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
