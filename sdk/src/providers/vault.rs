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
                    .flat_map(|(mount, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|(url, _)| {
                                (url.path().split('/').nth(2)).expect("Missing env")
                            })
                            .into_iter()
                            .map(|(key, group)| {
                                let host = host.clone();
                                let extra_env = extra_env.clone();
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
                                        .envs(envs(&extra_env))
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
    fn test_add_valid_vault_scheme() {
        let mut p = Vault::new();
        assert!(
            p.add("vault://localhost/secret/myapp/password".to_string())
                .is_ok()
        );
    }

    #[test]
    fn test_add_rejects_wrong_scheme() {
        let mut p = Vault::new();
        assert!(p.add("doppler://host/proj/env/VAR".to_string()).is_err());
    }

    #[test]
    fn test_add_rejects_plain_value() {
        let mut p = Vault::new();
        assert!(p.add("plainvalue".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_fake_cli_single_field() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "vault",
            r#"echo '{"data":{"data":{"password":"s3cret"}}}'"#,
        );

        let mut p = Vault::new();
        p.add("vault://localhost/secret/myapp/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await.unwrap();
        assert_eq!(
            result
                .get("vault://localhost/secret/myapp/password")
                .unwrap(),
            "s3cret"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_fake_cli_multiple_fields_same_key() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "vault",
            r#"echo '{"data":{"data":{"password":"s3cret","api_key":"key123"}}}'"#,
        );

        let mut p = Vault::new();
        p.add("vault://localhost/secret/myapp/password".to_string())
            .unwrap();
        p.add("vault://localhost/secret/myapp/api_key".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await.unwrap();
        assert_eq!(
            result
                .get("vault://localhost/secret/myapp/password")
                .unwrap(),
            "s3cret"
        );
        assert_eq!(
            result
                .get("vault://localhost/secret/myapp/api_key")
                .unwrap(),
            "key123"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_cli_not_found() {
        let empty_bin = tempdir().unwrap();
        let mut p = Vault::new();
        p.add("vault://localhost/secret/myapp/password".to_string())
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
                .contains("Vault CLI not found")
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_malformed_json_error() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "vault", "echo 'not valid json'");

        let mut p = Vault::new();
        p.add("vault://localhost/secret/myapp/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Vault error"));
    }
}
