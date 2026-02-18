use anyhow::{Result, anyhow, bail};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::debug;
use serde::Deserialize;
use std::{collections::HashMap, fs::File, io::Write, path::Path};
use tempfile::tempdir;
use url::Url;

use crate::{Hydration, providers::envs};

use super::Provider;

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
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "infisical" => {
                self.urls.insert(url, value);
                Ok(())
            }
            _ => bail!("Not an infisical scheme"),
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
                            .into_group_map_by(|(url, _)| {
                                (url.path().split('/').nth(2)).expect("Missing env")
                            })
                            .into_iter()
                            .map(|(env, group)| {
                                let path_groups = group
                                    .iter()
                                    .map(|(url, value)| {
                                        let path_segments: Vec<&str> = url.path().split('/').collect();
                                        let variable = path_segments.last().expect("Missing variable");
                                        let path = if path_segments.len() > 4 {
                                            format!("/{}", path_segments[3..path_segments.len()-1].join("/"))
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
                                    let config = HashMap::from([
                                        ("workspaceId", project),
                                        ("defaultEnvironment", ""),
                                    ]);
                                    let config_path = temp_dir.path().join(".infisical.json");
                                    let mut file = File::create(config_path)?;
                                    write!(file, "{}", serde_json::to_string(&config)?)?;
                                    drop(file);

                                    for (path, variables) in path_groups {
                                        let cmd = [
                                            "infisical",
                                            "--domain",
                                            &format!("https://{}/api", host),
                                            "export",
                                            "--path",
                                            if path.is_empty() { "/" } else { &path },
                                            "--env",
                                            env,
                                            "--projectId",
                                            project,
                                            "--format",
                                            "json",
                                        ];

                                        let child = match Command::new(cmd[0])
                                            .args(&cmd[1..])
                                            .current_dir(temp_dir.path())
                                            .envs(envs(&extra_env))
                                            .stdout(Stdio::piped())
                                            .stderr(Stdio::piped())
                                            .output()
                                            .await
                                        {
                                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                                bail!("Infisical CLI not found. Make sure the binary is in your PATH or install it from https://infisical.com/docs/cli/overview.")
                                            },
                                            Err(e) => {
                                                bail!("Infisical error: {e}")
                                            },
                                            Ok(child) => child,
                                        };

                                        let loaded = serde_json::from_slice::<Vec<InfisicalExport>>(
                                            &child.stdout,
                                        )
                                        .map_err(|err| {
                                            let stderr = String::from_utf8_lossy(&child.stderr);
                                            if stderr.contains("login expired") {
                                                anyhow!(
                                                    "Login expired for Infisical instance {host}: {stderr}",
                                                )
                                            } else if stderr.contains("unable to validate environment") {
                                                anyhow!(
                                                    "Workspace seems not accessible from logged account on {host}: {stderr}",
                                                )
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
    fn test_add_valid_infisical_scheme() {
        let mut p = Infisical::new();
        assert!(
            p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
                .is_ok()
        );
    }

    #[test]
    fn test_add_rejects_wrong_scheme() {
        let mut p = Infisical::new();
        assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_fake_cli() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "infisical",
            r#"echo '[{"key":"MY_SECRET","value":"infisical_value","secretPath":"/"}]'"#,
        );

        let mut p = Infisical::new();
        p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await.unwrap();
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
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p.resolve(Path::new("."), &extra).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_cli_not_found() {
        let empty_bin = tempdir().unwrap();
        let mut p = Infisical::new();
        p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
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
                .contains("Infisical CLI not found")
        );
    }
}
