use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;

use crate::{
    config::{Config, LadeRule, NetworkBinding, saved_user},
    files::{remove_files, split_env_files, write_files},
    network::{self, AcquiredNetwork},
    provider_progress::{
        ProviderProgressRenderer, start_provider_progress, stop_provider_progress,
    },
};

/// Command-scoped access state. Network guards and temporary files are owned
/// here so every caller gets the same cleanup behavior.
pub struct AttachedAccess {
    pub env: HashMap<String, String>,
    pub warnings: Vec<String>,
    files: HashMap<PathBuf, HashMap<String, String>>,
    _network: AcquiredNetwork,
}

impl AttachedAccess {
    pub fn cleanup(&mut self) -> Result<()> {
        remove_files(&mut self.files.keys())?;
        self.files.clear();
        Ok(())
    }
}

impl Drop for AttachedAccess {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

pub async fn acquire_attached(
    config: &Config,
    rules: &[(PathBuf, LadeRule)],
    rich_progress: bool,
) -> Result<AttachedAccess> {
    let saved_user = saved_user().await?;
    let network_bindings: Vec<NetworkBinding> =
        Config::network_bindings_from_rules(rules, &saved_user)?;
    let (vars, _sources, _maskable, warnings) = config.hydrate_rules(rules, &saved_user).await?;
    let mut progress: Option<ProviderProgressRenderer> =
        Some(start_provider_progress(rich_progress));
    let network_sink = progress.as_ref().expect("progress renderer").sink();
    let network = tokio::task::spawn_blocking(move || {
        network::start_attached_network_session(&network_bindings, network_sink)
    });
    let network = network
        .await
        .map_err(|error| anyhow::anyhow!("network task join error: {error}"));
    stop_provider_progress(&mut progress);
    let network = network??;
    let (mut env, files) = split_env_files(vars);
    for (key, value) in &network.env {
        match env.get(key) {
            Some(existing) if existing != value => {
                anyhow::bail!("conflicting binding '{key}' between secret and network providers")
            }
            Some(_) => {}
            None => {
                env.insert(key.clone(), value.clone());
            }
        }
    }
    write_files(&files)?;
    Ok(AttachedAccess {
        env,
        warnings,
        files,
        _network: network,
    })
}
