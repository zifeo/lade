use std::{collections::HashMap, path::Path};

use anyhow::{Result, bail};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::{AsyncWriteExt, future::try_join_all};
use itertools::Itertools;
use log::debug;
use url::Url;

use crate::{Hydration, providers::envs};

use super::{Provider, add_url};

static SEP: &str = "'Km5Ge8AbNc+QSBauOIN0jg'";

#[derive(Default)]
pub struct OnePassword {
    urls: HashMap<Url, String>,
}

impl OnePassword {
    pub fn new() -> Self {
        Default::default()
    }
}

#[async_trait]
impl Provider for OnePassword {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value, "op")
    }

    async fn resolve(&self, _: &Path, extra_env: &HashMap<String, String>) -> Result<Hydration> {
        let extra_env = extra_env.clone();
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
                let extra_env = extra_env.clone();
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
                        .envs(envs(&extra_env))
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .stdin(Stdio::piped())
                        .spawn()?;

                    debug!("stdin: {:?}", input);

                    let mut stdin = process.stdin.take().expect("Failed to open stdin");
                    stdin.write_all(input.as_bytes()).await?;
                    drop(stdin);

                    let child = match process.output().await {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            bail!("1Password CLI not found. Make sure the binary is in your PATH or install it from https://1password.com/downloads/command-line/.")
                        },
                        Err(e) => bail!("1Password error: {e}"),
                        Ok(child) => child,
                    };

                    let output = String::from_utf8_lossy(&child.stdout).trim().replace('\n', "\\n");
                    let errors = String::from_utf8_lossy(&child.stderr);

                    if errors.contains("[ERROR]") {
                        bail!("1Password error: {errors}")
                    }

                    debug!("stdout: {:?}", output);
                    debug!("stderr: {:?}", errors);
                    let loaded = output.split(SEP).collect::<Vec<_>>();

                    if loaded.len() != vars.len() {
                        bail!("1Password error: {errors}")
                    }

                    let hydration = vars
                        .iter().zip_eq(loaded)
                        .map(|((_, key), value)| (key.clone(), value.to_string()))
                        .collect::<Hydration>();

                    debug!("hydration: {:?}", hydration);
                    Ok(hydration)
                }
            })
            .collect::<Vec<_>>();

        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::fake_cli;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn test_add_valid_op_scheme() {
        let mut p = OnePassword::new();
        assert!(
            p.add("op://my.1password.com/vault_uuid/item_uuid/password".to_string())
                .is_ok()
        );
    }

    #[test]
    fn test_add_rejects_wrong_scheme() {
        let mut p = OnePassword::new();
        assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_fake_cli_single_secret() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "op", "printf 'op_secret_value'");

        let mut p = OnePassword::new();
        p.add("op://my.1password.com/vault_uuid/item_uuid/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await.unwrap();
        assert_eq!(
            result
                .get("op://my.1password.com/vault_uuid/item_uuid/password")
                .unwrap(),
            "op_secret_value"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_error_in_stderr() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "op", "echo '[ERROR] authentication failed' >&2");

        let mut p = OnePassword::new();
        p.add("op://my.1password.com/vault_uuid/item_uuid/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("1Password error"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_cli_not_found() {
        let empty_bin = tempdir().unwrap();
        let mut p = OnePassword::new();
        p.add("op://my.1password.com/vault_uuid/item_uuid/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            empty_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await;
        assert!(result.is_err());
    }
}
