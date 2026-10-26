use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::try_join_all;

use super::{Provider, Warnings, env_lookup, run_cli};
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

// A mise shim standing in for the shell drops pinned dirs from PATH before
// the snippet runs, so the snippet sets PATH again first.
fn with_path(bin: &str, path: &str, cmd: &str) -> String {
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    match bin {
        "fish" => format!("set --global --export PATH {quoted}\n{cmd}"),
        _ => format!("export PATH={quoted}\n{cmd}"),
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

    fn timeout(&self) -> std::time::Duration {
        super::PROVIDER_TIMEOUT
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn install_url(&self) -> &'static str {
        self.install_url
    }

    fn batch_unit(&self) -> &'static str {
        "command"
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
        let path = env_lookup(extra_env, &["PATH"]);
        let extra_env = Arc::new(extra_env.clone());
        let name = self.name();
        let install_url = self.install_url();
        let bin = self.bin;

        let fetches = self.commands.iter().map(|(full_value, cmd)| {
            let script = match &path {
                Some(path) => with_path(bin, path, cmd),
                None => cmd.clone(),
            };
            let full_value_str = full_value.clone();
            let extra_env = Arc::clone(&extra_env);
            async move {
                let output = run_cli(
                    &invocation(bin, &script),
                    &extra_env,
                    name,
                    install_url,
                    None,
                )
                .await?;
                // Never `require_cli_ok`: it falls back to stdout, and here stdout is
                // the secret.
                if !output.status.success() {
                    bail!(
                        "{name} command failed ({}): {}",
                        output.status,
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }

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
    async fn test_resolve_fails_when_the_command_fails() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "sh", "echo 'not logged in' >&2; exit 3");
        let mut p = Shell::new("sh", "sh", "url");
        p.add("sh://gcloud auth print-access-token".to_string())
            .unwrap();
        let err = p
            .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not logged in"), "{err}");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_failed_command_never_shows_its_stdout() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "sh", "printf hunter2; exit 1");
        let mut p = Shell::new("sh", "sh", "url");
        p.add("sh://op read op://v/i/f && false".to_string())
            .unwrap();
        let err = p
            .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
            .await
            .unwrap_err();
        assert!(!err.to_string().contains("hunter2"), "{err}");
    }

    #[test]
    fn test_with_path_sets_path_per_shell() {
        assert_eq!(
            with_path("fish", "/opt/acme/bin:/usr/bin", "echo hi"),
            "set --global --export PATH '/opt/acme/bin:/usr/bin'\necho hi"
        );
        assert_eq!(
            with_path("zsh", "/opt/it's/bin", "echo hi"),
            "export PATH='/opt/it'\\''s/bin'\necho hi"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_keeps_path_through_a_shim() {
        let shim = tempdir().unwrap();
        // Like a mise shim: rewrites PATH, then runs the real shell.
        fake_cli(&shim, "sh", "PATH=/usr/bin:/bin exec /bin/sh \"$@\"");
        let path = format!("{}:/usr/bin:/bin", shim.path().display());
        let mut p = Shell::new("sh", "sh", "url");
        p.add("sh://printf %s \"$PATH\"".to_string()).unwrap();
        let extra = HashMap::from([("PATH".to_string(), path.clone())]);
        let result = p
            .resolve(Path::new("."), &extra, &Warnings::default())
            .await
            .unwrap();
        assert_eq!(result.get("sh://printf %s \"$PATH\"").unwrap(), &path);
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
        // A mise shim for fish re-reads mise config under this fake HOME and
        // exits before fish runs, so the test needs the real binary.
        let fish_dir = std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
            .filter(|dir| !dir.ends_with("shims"))
            .find(|dir| dir.join("fish").is_file())
            .expect("fish on PATH outside a mise shims dir");
        let mut p = Shell::new("fish", "fish", "url");
        p.add("fish://printf %s \"$TOKEN\"".to_string()).unwrap();
        let extra = HashMap::from([
            ("HOME".to_string(), home.path().display().to_string()),
            ("PATH".to_string(), fish_dir.display().to_string()),
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
}
