use std::{collections::HashMap, path::Path};

use anyhow::{Ok, Result};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::debug;
use serde::Deserialize;
use url::Url;

use crate::Hydration;

use super::{Provider, add_url, deserialize_output, host_with_port, run_cli};

const NAME: &str = "Doppler";
const INSTALL_URL: &str = "https://docs.doppler.com/docs/install-cli";

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
        add_url(&mut self.urls, value, "doppler")
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
                    .flat_map(|(project, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|(url, _)| {
                                url.path().split('/').nth(2).expect("Missing env")
                            })
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
                                    let child =
                                        run_cli(&cmd, &extra_env, NAME, INSTALL_URL, None).await?;
                                    let loaded: HashMap<String, DopplerExport> =
                                        deserialize_output(&child, NAME)?;
                                    let hydration = vars
                                        .into_iter()
                                        .map(|(key, value)| {
                                            (
                                                value,
                                                loaded
                                                    .get(key)
                                                    .unwrap_or_else(|| {
                                                        panic!(
                                                            "Variable not found in Doppler: {}",
                                                            key
                                                        )
                                                    })
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
        let mut p = Doppler::new();
        assert!(
            p.add("doppler://api.doppler.com/myproject/dev/MY_SECRET".to_string())
                .is_ok()
        );
        assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
        assert!(p.add("plainvalue".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_single_var() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "doppler",
            r#"echo '{"MY_SECRET":{"computed":"doppler_value"}}'"#,
        );
        let mut p = Doppler::new();
        p.add("doppler://api.doppler.com/myproject/dev/MY_SECRET".to_string())
            .unwrap();
        let result = p
            .resolve(Path::new("."), &path_env(&fake_bin))
            .await
            .unwrap();
        assert_eq!(
            result
                .get("doppler://api.doppler.com/myproject/dev/MY_SECRET")
                .unwrap(),
            "doppler_value"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_multiple_vars_same_project() {
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
        let result = p
            .resolve(Path::new("."), &path_env(&fake_bin))
            .await
            .unwrap();
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
        let result = p.resolve(Path::new("."), &path_env(&empty_bin)).await;
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
        let result = p.resolve(Path::new("."), &path_env(&fake_bin)).await;
        assert!(result.unwrap_err().to_string().contains("Doppler error"));
    }
}
