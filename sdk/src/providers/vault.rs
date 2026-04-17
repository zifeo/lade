use anyhow::{Ok, Result};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::debug;
use serde::Deserialize;
use std::{collections::HashMap, path::Path};
use url::Url;

use crate::Hydration;

use super::{Provider, add_url, deserialize_output, host_with_port, run_cli};

const NAME: &str = "Vault";
const INSTALL_URL: &str = "https://developer.hashicorp.com/vault/docs/commands";

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
        add_url(&mut self.urls, value, "vault")
    }

    async fn resolve(&self, _: &Path, extra_env: &HashMap<String, String>) -> Result<Hydration> {
        let extra_env = extra_env.clone();
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|(url, _)| host_with_port(url))
            .into_iter()
            .flat_map(|(host, group)| {
                group
                    .into_iter()
                    .into_group_map_by(|(url, _)| {
                        url.path().split('/').nth(1).expect("Missing project")
                    })
                    .into_iter()
                    .flat_map(|(mount, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|(url, _)| {
                                url.path().split('/').nth(2).expect("Missing env")
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
                                        &urlencoding::decode(key)
                                            .expect("Invalid URL key decoding"),
                                    ];
                                    debug!("Lade run: {}", cmd.join(" "));
                                    let child =
                                        run_cli(&cmd, &extra_env, NAME, INSTALL_URL, None).await?;
                                    let loaded: VaultExport = deserialize_output(&child, NAME)?;
                                    let loaded = loaded.data.data;
                                    let hydration = group
                                        .into_iter()
                                        .map(|(url, value)| {
                                            let var = url
                                                .path()
                                                .split('/')
                                                .nth(3)
                                                .expect("Missing variable");
                                            (
                                                value.clone(),
                                                loaded
                                                    .get(
                                                        urlencoding::decode(var)
                                                            .expect("Invalid URL field decoding")
                                                            .as_ref(),
                                                    )
                                                    .unwrap_or_else(|| {
                                                        panic!(
                                                            "Variable not found in Vault: {}",
                                                            key
                                                        )
                                                    })
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
    use crate::providers::fake_cli;
    use std::path::Path;
    use tempfile::tempdir;

    fn path_env(dir: &tempfile::TempDir) -> HashMap<String, String> {
        HashMap::from([(
            "PATH".to_string(),
            dir.path().to_string_lossy().into_owned(),
        )])
    }

    #[test]
    fn test_add_routing() {
        let mut p = Vault::new();
        assert!(
            p.add("vault://localhost/secret/myapp/password".to_string())
                .is_ok()
        );
        assert!(p.add("doppler://host/proj/env/VAR".to_string()).is_err());
        assert!(p.add("plainvalue".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_single_field() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "vault",
            r#"echo '{"data":{"data":{"password":"s3cret"}}}'"#,
        );
        let mut p = Vault::new();
        p.add("vault://localhost/secret/myapp/password".to_string())
            .unwrap();
        let result = p
            .resolve(Path::new("."), &path_env(&fake_bin))
            .await
            .unwrap();
        assert_eq!(
            result
                .get("vault://localhost/secret/myapp/password")
                .unwrap(),
            "s3cret"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_multiple_fields_same_key() {
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
        let result = p
            .resolve(Path::new("."), &path_env(&fake_bin))
            .await
            .unwrap();
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
        let result = p.resolve(Path::new("."), &path_env(&empty_bin)).await;
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
        let result = p.resolve(Path::new("."), &path_env(&fake_bin)).await;
        assert!(result.unwrap_err().to_string().contains("Vault error"));
    }
}
