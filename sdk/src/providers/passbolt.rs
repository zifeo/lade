use std::{collections::HashMap, path::Path};

use anyhow::{Result, anyhow, bail};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::future::try_join_all;

use itertools::Itertools;
use log::debug;
use url::Url;

use crate::{Hydration, providers::envs};

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

    async fn resolve(&self, _: &Path, extra_env: &HashMap<String, String>) -> Result<Hydration> {
        let extra_env = extra_env.clone();
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|(url, _)| url.host().expect("Missing host"))
            .into_iter()
            .flat_map(|(host, group)| {
                let extra_env = extra_env.clone();
                group
                    .into_iter()
                    .into_group_map_by(|(url, _)| url.path().split('/').nth(1).expect("Missing resource id"))
                    .into_iter()
                    .map(move |(resource_id, group)| {
                        let host = host.clone();
                        let extra_env = extra_env.clone();
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
                                .envs(envs(&extra_env))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::tempdir;

    #[cfg(unix)]
    fn fake_cli(dir: &tempfile::TempDir, name: &str, script_body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn test_add_valid_passbolt_scheme() {
        let mut p = Passbolt::new();
        assert!(
            p.add("passbolt://passbolt.example.com/resource-uuid/password".to_string())
                .is_ok()
        );
    }

    #[test]
    fn test_add_rejects_wrong_scheme() {
        let mut p = Passbolt::new();
        assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_fake_cli() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "passbolt",
            r#"echo '{"password":"passbolt_value","username":"user"}'"#,
        );

        let mut p = Passbolt::new();
        p.add("passbolt://passbolt.example.com/resource-uuid/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await.unwrap();
        assert_eq!(
            result
                .get("passbolt://passbolt.example.com/resource-uuid/password")
                .unwrap(),
            "passbolt_value"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_malformed_json_error() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "passbolt", "echo 'not valid json'");

        let mut p = Passbolt::new();
        p.add("passbolt://passbolt.example.com/resource-uuid/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Passbolt error"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_cli_not_found() {
        let empty_bin = tempdir().unwrap();
        let mut p = Passbolt::new();
        p.add("passbolt://passbolt.example.com/resource-uuid/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            empty_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Passbolt CLI not found")
        );
    }
}
