use std::{collections::HashMap, path::Path};

use anyhow::{Result, anyhow, bail};
use async_process::{Command, Stdio};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use url::Url;

use crate::Hydration;

mod doppler;
mod file;
mod infisical;
mod onepassword;
mod passbolt;
mod raw;
mod vault;

#[async_trait]
pub trait Provider: Sync {
    fn add(&mut self, value: String) -> Result<()>;
    async fn resolve(&self, cwd: &Path, extra_env: &HashMap<String, String>) -> Result<Hydration>;
}

pub fn providers() -> Vec<Box<dyn Provider + Send>> {
    vec![
        Box::new(doppler::Doppler::new()),
        Box::new(infisical::Infisical::new()),
        Box::new(onepassword::OnePassword::new()),
        Box::new(vault::Vault::new()),
        Box::new(passbolt::Passbolt::new()),
        Box::new(file::File::new()),
        Box::new(raw::Raw::new()),
    ]
}

pub fn envs(extra: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    env.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    env
}

pub fn add_url(urls: &mut HashMap<Url, String>, value: String, scheme: &str) -> Result<()> {
    match Url::parse(&value) {
        Ok(url) if url.scheme() == scheme => {
            urls.insert(url, value);
            Ok(())
        }
        _ => bail!("Not a {scheme} scheme"),
    }
}

pub async fn run_cli(
    cmd: &[&str],
    extra_env: &HashMap<String, String>,
    name: &str,
    install_url: &str,
    cwd: Option<&Path>,
) -> Result<std::process::Output> {
    let mut c = Command::new(cmd[0]);
    c.args(&cmd[1..])
        .envs(envs(extra_env))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    c.output().await.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => anyhow!(
            "{name} CLI not found. Make sure the binary is in your PATH or install it from {install_url}."
        ),
        _ => anyhow!("{name} error: {e}"),
    })
}

pub fn deserialize_output<T: DeserializeOwned>(
    output: &std::process::Output,
    name: &str,
) -> Result<T> {
    serde_json::from_slice(&output.stdout).map_err(|err| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow!("{name} error: {err} (stderr: {stderr})")
    })
}

pub fn host_with_port(url: &Url) -> String {
    match url.port() {
        Some(port) => format!("{}:{}", url.host().expect("Missing host"), port),
        None => url.host().expect("Missing host").to_string(),
    }
}

#[cfg(test)]
pub fn fake_cli(dir: &tempfile::TempDir, name: &str, script_body: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
