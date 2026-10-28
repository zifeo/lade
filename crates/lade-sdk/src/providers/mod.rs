use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::Hydration;

pub mod add;
pub mod compat;
#[cfg(feature = "network")]
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
mod support;
pub mod terminal;
mod vault;

pub(crate) use support::*;

/// A provider is logged in or not: it answers within this or fails.
pub const PROVIDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// For a provider that can wait on a human: Touch ID, a hardware key touch.
pub const HUMAN_PROVIDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) fn provider_timeout_error(name: &str, limit: std::time::Duration) -> anyhow::Error {
    anyhow::anyhow!(
        "{name} did not answer within {}s. Check that it is unlocked and logged in.",
        limit.as_secs()
    )
}

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

    /// Cap on `resolve` and `search`. Past it the call fails and its CLI is killed.
    fn timeout(&self) -> std::time::Duration;

    /// Where to install/find the backing tool or product docs.
    fn install_url(&self) -> &'static str;

    fn transport(&self) -> Transport {
        Transport::Cli
    }

    /// Titles or paths a wizard can pick. Empty when this provider has no list CLI.
    fn search(&self, extra_env: &HashMap<String, String>) -> Result<Vec<String>> {
        let _ = extra_env;
        Ok(Vec::new())
    }

    /// URI the `lade add` wizard suggests for a picked secret.
    fn compose_add_uri(&self, _picked: &str, _extras: &HashMap<String, String>) -> Option<String> {
        None
    }

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

    /// Name of the provider that takes `value`, or `None` when `add` sends it
    /// to raw. A value its provider rejects still belongs to that provider.
    pub fn claimed_by(value: &str) -> Option<&'static str> {
        let (scheme, _) = value.split_once("://")?;
        let mut providers = Self::new();
        if providers.add(value.to_string()).is_ok() && providers.fallback.has_work() {
            return None;
        }
        providers.provider(scheme).map(|p| p.name())
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
            let started = std::time::Instant::now();
            let hydration =
                tokio::time::timeout(p.timeout(), p.resolve(cwd, extra_env, warnings)).await;
            log::info!(
                target: "lade::timing",
                "provider {}: {:.0} ms",
                p.name(),
                started.elapsed().as_secs_f64() * 1000.0
            );
            let hydration =
                hydration.map_err(|_| provider_timeout_error(p.name(), p.timeout()))??;
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

pub fn compose_secret_add_uri(
    scheme: &str,
    picked: &str,
    extras: &HashMap<String, String>,
) -> Option<String> {
    Providers::new()
        .provider(scheme)?
        .compose_add_uri(picked, extras)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub use tests::fake_cli;
