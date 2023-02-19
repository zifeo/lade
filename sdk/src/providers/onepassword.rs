use std::collections::HashMap;

use anyhow::{bail, Ok, Result};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::AsyncWriteExt;

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
        let vars = self
            .urls
            .iter()
            .enumerate()
            .map(|(idx, u)| (idx.to_string(), u.to_string()))
            .collect::<HashMap<_, _>>();

        if vars.is_empty() {
            return Ok(HashMap::new());
        }

        let json = serde_json::to_string(&vars)?;

        let f = async move {
            let cmd = &["op", "inject"];
            info!("{}", cmd.join(" "));
            let mut child = Command::new(cmd[0])
                .args(&cmd[1..])
                .stdout(Stdio::piped())
                .stdin(Stdio::piped())
                .spawn()?;

            debug!("stdin: {:?}", json);

            let mut stdin = child.stdin.take().expect("Failed to open stdin");
            stdin.write_all(json.as_bytes()).await?;
            drop(stdin);

            let loaded = serde_json::from_slice::<Hydration>(&child.output().await?.stdout)?;

            let hydration = vars
                .into_iter()
                .map(|(key, var)| (var, loaded.get(&key).expect("Variable not found").clone()))
                .collect::<Hydration>();

            debug!("hydration: {:?}", hydration);
            Ok(hydration)
        };

        Ok(f.await?)
    }
}
