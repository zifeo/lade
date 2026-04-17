use std::{collections::HashMap, path::Path};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;
use std::process::Stdio;
use tokio::process::Command;
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

    fn has_work(&self) -> bool {
        true
    }

    async fn resolve(&self, cwd: &Path, extra_env: &HashMap<String, String>) -> Result<Hydration>;
}

pub struct Providers {
    by_scheme: FxHashMap<&'static str, Box<dyn Provider + Send>>,
    fallback: Box<dyn Provider + Send>,
}

impl Providers {
    pub fn new() -> Self {
        let mut by_scheme: FxHashMap<&'static str, Box<dyn Provider + Send>> = FxHashMap::default();
        by_scheme.insert("doppler", Box::new(doppler::Doppler::new()));
        by_scheme.insert("infisical", Box::new(infisical::Infisical::new()));
        by_scheme.insert("op", Box::new(onepassword::OnePassword::new()));
        by_scheme.insert("vault", Box::new(vault::Vault::new()));
        by_scheme.insert("passbolt", Box::new(passbolt::Passbolt::new()));
        by_scheme.insert("file", Box::new(file::File::new()));
        Self {
            by_scheme,
            fallback: Box::new(raw::Raw::new()),
        }
    }

    pub fn add(&mut self, value: String) -> Result<()> {
        let scheme = value.split_once("://").map(|(s, _)| s).unwrap_or("");
        match self.by_scheme.get_mut(scheme) {
            Some(p) => match p.add(value.clone()) {
                Ok(()) => Ok(()),
                // Preserve today's behaviour: a scheme-specific provider that rejects the value
                // (e.g. file:// without ?query=) falls back to Raw rather than returning an error.
                Err(_) => self.fallback.add(value),
            },
            None => self.fallback.add(value),
        }
    }

    pub async fn resolve(
        &self,
        cwd: &Path,
        extra_env: &HashMap<String, String>,
    ) -> Result<Hydration> {
        let active: Vec<&dyn Provider> = self
            .by_scheme
            .values()
            .map(|p| p.as_ref() as &dyn Provider)
            .chain(std::iter::once(self.fallback.as_ref() as &dyn Provider))
            .filter(|p| p.has_work())
            .collect();
        Ok(
            try_join_all(active.iter().map(|p| p.resolve(cwd, extra_env)))
                .await?
                .into_iter()
                .flatten()
                .collect::<Hydration>(),
        )
    }
}

pub fn add_url(urls: &mut FxHashMap<Url, String>, value: String, scheme: &str) -> Result<()> {
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
        .envs(std::env::vars())
        .envs(extra_env.iter())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn has_work_for(scheme: &str, uri: &str) -> bool {
        let mut p = Providers::new();
        p.add(uri.to_string()).unwrap();
        p.by_scheme
            .get(scheme)
            .map(|prov| prov.has_work())
            .unwrap_or(false)
    }

    fn fallback_has_work(uri: &str) -> bool {
        let mut p = Providers::new();
        p.add(uri.to_string()).unwrap();
        p.fallback.has_work()
    }

    #[test]
    fn test_dispatch_doppler() {
        assert!(has_work_for(
            "doppler",
            "doppler://api.doppler.com/proj/env/KEY"
        ));
        assert!(!fallback_has_work("doppler://api.doppler.com/proj/env/KEY"));
    }

    #[test]
    fn test_dispatch_vault() {
        assert!(has_work_for("vault", "vault://localhost/secret/app/pass"));
        assert!(!fallback_has_work("vault://localhost/secret/app/pass"));
    }

    #[test]
    fn test_dispatch_op() {
        assert!(has_work_for("op", "op://my.1password.com/vault/item/field"));
        assert!(!fallback_has_work("op://my.1password.com/vault/item/field"));
    }

    #[test]
    fn test_dispatch_plain_value_to_fallback() {
        assert!(fallback_has_work("plainvalue"));
    }

    #[test]
    fn test_dispatch_bang_escaped_to_fallback() {
        assert!(fallback_has_work("!escaped"));
    }

    #[test]
    fn test_dispatch_unknown_scheme_to_fallback() {
        assert!(fallback_has_work("foo://some/path"));
    }

    #[test]
    fn test_dispatch_file_without_query_falls_back_to_raw() {
        // file:// without ?query= is rejected by File::add and must land on Raw.
        assert!(fallback_has_work("file:///path/to/config.json"));
        assert!(!has_work_for("file", "file:///path/to/config.json"));
    }

    #[test]
    fn test_dispatch_file_with_query_goes_to_file() {
        assert!(has_work_for(
            "file",
            "file:///path/to/config.json?query=.key"
        ));
        assert!(!fallback_has_work("file:///path/to/config.json?query=.key"));
    }
}
