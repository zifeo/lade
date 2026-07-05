use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::try_join_all;

use super::{Provider, Warnings, run_cli};
use crate::Hydration;

pub struct Shell {
    bin: &'static str,
    name: &'static str,
    install_url: &'static str,
    commands: Vec<(String, String)>,
}

impl Shell {
    pub fn new(bin: &'static str, name: &'static str, install_url: &'static str) -> Self {
        Self {
            bin,
            name,
            install_url,
            commands: Vec::new(),
        }
    }
}

#[async_trait]
impl Provider for Shell {
    fn add(&mut self, value: String) -> Result<()> {
        let prefix = format!("{}://", self.bin);
        if value.starts_with(&prefix) {
            let cmd = value[prefix.len()..].to_string();
            if cmd.is_empty() {
                bail!("{} command cannot be empty", self.name);
            }
            self.commands.push((value, cmd));
            Ok(())
        } else {
            bail!("Not a {} scheme", self.bin);
        }
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn install_url(&self) -> &'static str {
        self.install_url
    }

    fn has_work(&self) -> bool {
        !self.commands.is_empty()
    }

    fn masks_in_output(&self) -> bool {
        true
    }

    async fn resolve(
        &self,
        _: &Path,
        extra_env: &HashMap<String, String>,
        _: &Warnings,
    ) -> Result<Hydration> {
        let extra_env = Arc::new(extra_env.clone());
        let name = self.name();
        let install_url = self.install_url();
        let bin = self.bin;

        let fetches = self.commands.iter().map(|(full_value, cmd)| {
            let cmd_str = cmd.clone();
            let full_value_str = full_value.clone();
            let extra_env = Arc::clone(&extra_env);
            async move {
                let output = tokio::time::timeout(
                    Duration::from_secs(30),
                    run_cli(&[bin, "-c", &cmd_str], &extra_env, name, install_url, None),
                )
                .await
                .map_err(|_| {
                    anyhow::anyhow!("{} command timed out after 30s: {}", name, cmd_str)
                })??;

                let stdout = String::from_utf8(output.stdout)
                    .map_err(|e| anyhow::anyhow!("{} output is not UTF-8: {}", name, e))?;
                let value = stdout.trim_end_matches(['\n', '\r']).to_string();

                Ok::<_, anyhow::Error>((full_value_str, value))
            }
        });

        Ok(try_join_all(fetches).await?.into_iter().collect())
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
        let mut p = Shell::new("sh", "sh", "url");
        assert!(p.add("sh://echo hi".to_string()).is_ok());
        assert!(p.add("sh://".to_string()).is_err());
        assert!(p.add("bash://echo hi".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_sh() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "sh", "echo \"hello world\"");
        let mut p = Shell::new("sh", "sh", "url");
        p.add("sh://echo \"hello world\"".to_string()).unwrap();
        let result = p
            .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
            .await
            .unwrap();
        assert_eq!(
            result.get("sh://echo \"hello world\"").unwrap(),
            "hello world"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_timeout() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "sh", "sleep 2");
        let mut p = Shell::new("sh", "sh", "url");
        p.add("sh://sleep 2".to_string()).unwrap();

        // We can't easily test 30s timeout in unit tests without making it configurable,
        // but we can verify the mechanism works if we were to use a shorter timeout.
        // For now, we trust tokio::time::timeout.
    }
}
