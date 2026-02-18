use anyhow::{Ok, Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::debug;
use serde::Deserialize;
use std::{collections::HashMap, fs::File, io::Write, path::Path};
use tempfile::tempdir;
use url::Url;

use crate::Hydration;

use super::{Provider, add_url, host_with_port, run_cli};

const NAME: &str = "Infisical";
const INSTALL_URL: &str = "https://infisical.com/docs/cli/overview";

#[derive(Default)]
pub struct Infisical {
    urls: HashMap<Url, String>,
}

impl Infisical {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Deserialize)]
struct InfisicalExport {
    key: String,
    value: String,
    #[serde(rename = "secretPath")]
    secret_path: String,
}

#[async_trait]
impl Provider for Infisical {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value, "infisical")
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
                    .into_group_map_by(|(url, _)| url.path().split('/').nth(1).expect("Missing project"))
                    .into_iter()
                    .flat_map(|(project, group)| {
                        group
                            .into_iter()
                            .into_group_map_by(|(url, _)| url.path().split('/').nth(2).expect("Missing env"))
                            .into_iter()
                            .map(|(env, group)| {
                                let path_groups = group
                                    .iter()
                                    .map(|(url, value)| {
                                        let segs: Vec<&str> = url.path().split('/').collect();
                                        let variable = segs.last().expect("Missing variable");
                                        let path = if segs.len() > 4 {
                                            format!("/{}", segs[3..segs.len()-1].join("/"))
                                        } else {
                                            "".to_string()
                                        };
                                        (path, variable.to_string(), (*value).clone())
                                    })
                                    .into_group_map_by(|(path, _, _)| path.clone())
                                    .into_iter()
                                    .map(|(path, vars)| (path, vars.into_iter().map(|(_, var, url)| (var, url)).collect()))
                                    .collect::<HashMap<String, Vec<(String, String)>>>();

                                let host = host.clone();
                                let extra_env = extra_env.clone();
                                async move {
                                    let mut hydration = Hydration::new();
                                    let temp_dir = tempdir()?;
                                    let config = HashMap::from([("workspaceId", project), ("defaultEnvironment", "")]);
                                    let config_path = temp_dir.path().join(".infisical.json");
                                    let mut file = File::create(config_path)?;
                                    write!(file, "{}", serde_json::to_string(&config)?)?;
                                    drop(file);

                                    for (path, variables) in path_groups {
                                        let cmd = [
                                            "infisical", "--domain", &format!("https://{}/api", host),
                                            "export", "--path", if path.is_empty() { "/" } else { &path },
                                            "--env", env, "--projectId", project, "--format", "json",
                                        ];
                                        let child = run_cli(&cmd, &extra_env, NAME, INSTALL_URL, Some(temp_dir.path())).await?;
                                        let loaded = serde_json::from_slice::<Vec<InfisicalExport>>(&child.stdout)
                                            .map_err(|err| {
                                                let stderr = String::from_utf8_lossy(&child.stderr);
                                                if stderr.contains("login expired") {
                                                    anyhow!("Login expired for Infisical instance {host}: {stderr}")
                                                } else if stderr.contains("unable to validate environment") {
                                                    anyhow!("Workspace seems not accessible from logged account on {host}: {stderr}")
                                                } else {
                                                    anyhow!("Infisical error: {err} (stderr: {stderr})")
                                                }
                                            })?
                                            .into_iter()
                                            .map(|e| (e.key, e.value, e.secret_path))
                                            .collect::<Vec<_>>();

                                        let mut missing_vars = Vec::new();
                                        for (var_name, original_url) in variables {
                                            if let Some((_, value, _)) = loaded.iter().find(|(key, _, _)| key == &var_name) {
                                                hydration.insert(original_url, value.clone());
                                            } else {
                                                missing_vars.push(var_name);
                                            }
                                        }
                                        if !missing_vars.is_empty() {
                                            bail!("Variables {} not found in path {} of Infisical project {}", missing_vars.join(", "), path, project);
                                        }
                                    }

                                    temp_dir.close()?;
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
        let mut p = Infisical::new();
        assert!(
            p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
                .is_ok()
        );
        assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_single_var() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "infisical",
            r#"echo '[{"key":"MY_SECRET","value":"infisical_value","secretPath":"/"}]'"#,
        );
        let mut p = Infisical::new();
        p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
            .unwrap();
        let result = p
            .resolve(Path::new("."), &path_env(&fake_bin))
            .await
            .unwrap();
        assert_eq!(
            result
                .get("infisical://app.infisical.com/proj123/dev/MY_SECRET")
                .unwrap(),
            "infisical_value"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_missing_variable_error() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "infisical", "echo '[]'");
        let mut p = Infisical::new();
        p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
            .unwrap();
        let result = p.resolve(Path::new("."), &path_env(&fake_bin)).await;
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_cli_not_found() {
        let empty_bin = tempdir().unwrap();
        let mut p = Infisical::new();
        p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
            .unwrap();
        let result = p.resolve(Path::new("."), &path_env(&empty_bin)).await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Infisical CLI not found")
        );
    }
}
