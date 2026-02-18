use std::{collections::HashMap, path::Path};

use crate::{Hydration, providers::envs};
use anyhow::{Result, anyhow, bail};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::debug;
use serde::Deserialize;
use url::Url;

use super::Provider;

#[derive(Default)]
pub struct Doppler {
    urls: HashMap<Url, String>,
}

impl Doppler {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Deserialize)]
struct DopplerExport {
    computed: String,
}

#[async_trait]
impl Provider for Doppler {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "doppler" => {
                self.urls.insert(url, value);
                Ok(())
            }
            _ => bail!("Not a doppler scheme"),
        }
    }
    async fn resolve(&self, _: &Path, extra_env: &HashMap<String, String>) -> Result<Hydration> {
        let extra_env = extra_env.clone();
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
                    .flat_map(|(project, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|(url, _)| url.path().split('/').nth(2).expect("Missing env"))
                            .into_iter()
                            .map(|(env, group)| {
                                let vars = group
                                    .iter()
                                    .map(|(u, value)| {
                                        (
                                            u.path().split('/').nth(3).expect("Missing variable"),
                                            (*value).clone(),
                                        )
                                    })
                                    .collect::<HashMap<_, _>>();

                                let host = host.clone();
                                let extra_env = extra_env.clone();
                                async move {
                                    let cmd = [
                                        "doppler",
                                        "--api-host",
                                        &format!("https://{}", host),
                                        "secrets",
                                        "--project",
                                        project,
                                        "--config",
                                        env,
                                        "--json",
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
                                                bail!("Doppler CLI not found. Make sure the binary is in your PATH or install it from https://docs.doppler.com/docs/install-cli.")
                                            },
                                            Err(e) => {
                                                bail!("Doppler error: {e}")
                                            },
                                            Ok(child) => child,
                                        };

                                    let loaded = serde_json::from_slice::<
                                        HashMap<String, DopplerExport>,
                                    >(
                                        &child.stdout
                                    )
                                    .map_err(|err| {
                                        let stderr = String::from_utf8_lossy(&child.stderr);
                                        anyhow!("Doppler error: {err} (stderr: {stderr})",)
                                    })?;

                                    let hydration = vars
                                        .into_iter()
                                        .map(|(key, value)| {
                                            (
                                                value,
                                                loaded
                                                    .get(key)
                                                    .unwrap_or_else(|| panic!(
                                                        "Variable not found in Doppler: {}",
                                                        key
                                                    ))
                                                    .computed
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
    fn test_add_valid_doppler_scheme() {
        let mut p = Doppler::new();
        assert!(
            p.add("doppler://api.doppler.com/myproject/dev/MY_SECRET".to_string())
                .is_ok()
        );
    }

    #[test]
    fn test_add_rejects_wrong_scheme() {
        let mut p = Doppler::new();
        assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    }

    #[test]
    fn test_add_rejects_plain_value() {
        let mut p = Doppler::new();
        assert!(p.add("plainvalue".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_fake_cli() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "doppler",
            r#"echo '{"MY_SECRET":{"computed":"doppler_value"}}'"#,
        );

        let mut p = Doppler::new();
        p.add("doppler://api.doppler.com/myproject/dev/MY_SECRET".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await.unwrap();
        assert_eq!(
            result
                .get("doppler://api.doppler.com/myproject/dev/MY_SECRET")
                .unwrap(),
            "doppler_value"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_fake_cli_multiple_vars_same_project() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "doppler",
            r#"echo '{"KEY1":{"computed":"val1"},"KEY2":{"computed":"val2"}}'"#,
        );

        let mut p = Doppler::new();
        p.add("doppler://api.doppler.com/myproject/dev/KEY1".to_string())
            .unwrap();
        p.add("doppler://api.doppler.com/myproject/dev/KEY2".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await.unwrap();
        assert_eq!(
            result
                .get("doppler://api.doppler.com/myproject/dev/KEY1")
                .unwrap(),
            "val1"
        );
        assert_eq!(
            result
                .get("doppler://api.doppler.com/myproject/dev/KEY2")
                .unwrap(),
            "val2"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_cli_not_found() {
        let empty_bin = tempdir().unwrap();
        let mut p = Doppler::new();
        p.add("doppler://api.doppler.com/myproject/dev/MY_SECRET".to_string())
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
                .contains("Doppler CLI not found")
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_malformed_json_error() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "doppler", "echo 'not valid json'");

        let mut p = Doppler::new();
        p.add("doppler://api.doppler.com/myproject/dev/MY_SECRET".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Doppler error"));
    }
}
