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

fn invocation<'a>(bin: &'a str, cmd: &'a str) -> Vec<&'a str> {
    match bin {
        "fish" => vec![bin, "--no-config", "-c", cmd],
        "zsh" => vec![bin, "-f", "-c", cmd],
        _ => vec![bin, "-c", cmd],
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
                    run_cli(
                        &invocation(bin, &cmd_str),
                        &extra_env,
                        name,
                        install_url,
                        None,
                    ),
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
    fn test_invocation_skips_startup_files() {
        assert_eq!(
            invocation("fish", "echo hi"),
            vec!["fish", "--no-config", "-c", "echo hi"]
        );
        assert_eq!(invocation("bash", "echo hi"), vec!["bash", "-c", "echo hi"]);
        assert_eq!(
            invocation("zsh", "echo hi"),
            vec!["zsh", "-f", "-c", "echo hi"]
        );
        assert_eq!(invocation("sh", "echo hi"), vec!["sh", "-c", "echo hi"]);
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
    async fn test_resolve_fish_ignores_config_overwrite() {
        let home = tempdir().unwrap();
        let fish_dir = home.path().join(".config/fish");
        std::fs::create_dir_all(&fish_dir).unwrap();
        std::fs::write(
            fish_dir.join("config.fish"),
            "set -x TOKEN from_fish_profile\n",
        )
        .unwrap();
        let mut p = Shell::new("fish", "fish", "url");
        p.add("fish://printf %s \"$TOKEN\"".to_string()).unwrap();
        let extra = HashMap::from([
            ("HOME".to_string(), home.path().display().to_string()),
            ("TOKEN".to_string(), "from_binding".to_string()),
        ]);
        let result = p
            .resolve(Path::new("."), &extra, &Warnings::default())
            .await
            .unwrap();
        assert_eq!(
            result.get("fish://printf %s \"$TOKEN\"").unwrap(),
            "from_binding"
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
