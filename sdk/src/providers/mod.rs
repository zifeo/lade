use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::de::DeserializeOwned;
use std::process::Stdio;
use tokio::process::Command;
use url::Url;

use crate::Hydration;

pub mod compat;
pub mod network;

#[derive(Clone, Default)]
pub struct Warnings(Arc<Mutex<Vec<String>>>);

impl Warnings {
    pub fn push(&self, msg: impl Into<String>) {
        self.0.lock().unwrap().push(msg.into());
    }

    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}
mod age;
mod awssm;
mod azurekv;
mod bw;
mod doppler;
mod file;
mod gcpsm;
mod infisical;
mod onepassword;
mod params;
mod passbolt;
mod raw;
mod sh;
mod sops;
mod vault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Cli,
    Sdk,
}

#[async_trait]
pub trait Provider: Sync {
    fn add(&mut self, value: String) -> Result<()>;

    /// Human-readable provider name, used in user-facing messages.
    fn name(&self) -> &'static str;

    /// Where to install/find the backing tool or product docs.
    fn install_url(&self) -> &'static str;

    fn transport(&self) -> Transport;

    /// The key this provider groups on before one I/O call.
    fn batch_unit(&self) -> &'static str;

    fn has_work(&self) -> bool {
        true
    }

    /// Whether resolved values from this provider should be masked in subprocess output.
    fn masks_in_output(&self) -> bool {
        true
    }

    async fn resolve(
        &self,
        cwd: &Path,
        extra_env: &HashMap<String, String>,
        warnings: &Warnings,
    ) -> Result<Hydration>;
}

pub struct Providers {
    by_scheme: FxHashMap<&'static str, Box<dyn Provider + Send>>,
    fallback: Box<dyn Provider + Send>,
}

impl Default for Providers {
    fn default() -> Self {
        Self::new()
    }
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
        by_scheme.insert("awssm", Box::new(awssm::AwsSm::new()));
        by_scheme.insert("azurekv", Box::new(azurekv::AzureKv::new()));
        by_scheme.insert("bw", Box::new(bw::Bitwarden::new()));
        by_scheme.insert("gcpsm", Box::new(gcpsm::GcpSm::new()));
        by_scheme.insert("age", Box::new(age::Age::new()));
        by_scheme.insert("sops", Box::new(sops::Sops::new()));
        by_scheme.insert(
            "sh",
            Box::new(sh::Shell::new(
                "sh",
                "sh",
                "https://pubs.opengroup.org/onlinepubs/9699919799/utilities/sh.html",
            )),
        );
        by_scheme.insert(
            "bash",
            Box::new(sh::Shell::new(
                "bash",
                "bash",
                "https://www.gnu.org/software/bash/",
            )),
        );
        by_scheme.insert(
            "zsh",
            Box::new(sh::Shell::new("zsh", "zsh", "https://www.zsh.org/")),
        );
        by_scheme.insert(
            "fish",
            Box::new(sh::Shell::new("fish", "fish", "https://fishshell.com/")),
        );
        Self {
            by_scheme,
            fallback: Box::new(raw::Raw::new()),
        }
    }

    pub fn provider(&self, scheme: &str) -> Option<&(dyn Provider + Send)> {
        self.by_scheme.get(scheme).map(|p| p.as_ref())
    }

    pub fn registered_schemes(&self) -> Vec<&'static str> {
        let mut schemes: Vec<&'static str> = self.by_scheme.keys().copied().collect();
        schemes.sort_unstable();
        schemes
    }

    pub fn add(&mut self, value: String) -> Result<()> {
        let scheme = value.split_once("://").map(|(s, _)| s).unwrap_or("");
        match self.by_scheme.get_mut(scheme) {
            Some(p) => match p.add(value.clone()) {
                Ok(()) => Ok(()),
                Err(_) if scheme == "file" => self.fallback.add(value),
                Err(e) => Err(e),
            },
            None => self.fallback.add(value),
        }
    }

    pub async fn resolve(
        &self,
        cwd: &Path,
        extra_env: &HashMap<String, String>,
        warnings: &Warnings,
    ) -> Result<(Hydration, FxHashSet<String>)> {
        let active: Vec<&dyn Provider> = self
            .by_scheme
            .values()
            .map(|p| p.as_ref() as &dyn Provider)
            .chain(std::iter::once(self.fallback.as_ref() as &dyn Provider))
            .filter(|p| p.has_work())
            .collect();

        let results = try_join_all(active.iter().map(|p| async move {
            let hydration = p.resolve(cwd, extra_env, warnings).await?;
            Ok::<_, anyhow::Error>((p.masks_in_output(), hydration))
        }))
        .await?;

        let mut full_hydration = Hydration::default();
        let mut maskable_sources = FxHashSet::default();

        for (masks, hydration) in results {
            if masks {
                maskable_sources.extend(hydration.keys().cloned());
            }
            full_hydration.extend(hydration);
        }

        Ok((full_hydration, maskable_sources))
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
        .envs(extra_env.iter())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // bash -c sources $BASH_ENV even though it does not read .bashrc.
    if cmd[0] == "bash" {
        c.env_remove("BASH_ENV");
    }
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

pub fn env_lookup(extra_env: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = extra_env.get(*key)
            && !value.is_empty()
        {
            return Some(value.clone());
        }
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            return Some(value);
        }
    }
    None
}

pub fn json_query_string(raw: &str, query: Option<&str>) -> Result<String> {
    let Some(query) = query.filter(|q| !q.is_empty()) else {
        return Ok(raw.to_string());
    };
    let json: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| anyhow!("JSON query {query} failed: value is not JSON ({e})"))?;
    let compiled = access_json::JSONQuery::parse(query)
        .map_err(|e| anyhow!("cannot compile query {query}: {e}"))?;
    let res = compiled
        .execute(&json)
        .map_err(|e| anyhow!("query {query} failed: {e}"))?
        .ok_or_else(|| anyhow!("no query result for {query}"))?;
    Ok(match res {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    })
}

pub fn host_with_port(url: &Url) -> String {
    match url.port() {
        Some(port) => format!("{}:{}", url.host().expect("Missing host"), port),
        None => url.host().expect("Missing host").to_string(),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub use tests::fake_cli;
