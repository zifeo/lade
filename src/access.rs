use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::{
    config::{Config, LadeRule, ResolvedEntry, binding_name, resolve_entry, saved_user},
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
    pub cached: HashSet<String>,
    files: HashMap<PathBuf, HashMap<String, String>>,
    _network: AcquiredNetwork,
}

impl AttachedAccess {
    pub fn public_hydrate(&self) -> HashMap<String, String> {
        crate::wrap::public_hydrate(&self.env, &self.files)
    }

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
    patterned: &[(PathBuf, String, LadeRule)],
    cwd: &Path,
    rich_progress: bool,
) -> Result<AttachedAccess> {
    let saved_user = saved_user().await?;
    let network_bindings = Config::network_bindings_from_rules(rules, &saved_user);
    let hit = crate::hub::lookup(cwd, config.walk_hash(), &saved_user, patterned);
    let (mut vars, _sources, _maskable, warnings) = config
        .hydrate_rules_except(rules, &saved_user, &hit.keys)
        .await?;
    let mut output_for = HashMap::new();
    for (dir, rule) in rules {
        let output = rule
            .config
            .as_ref()
            .and_then(|config| config.file.clone())
            .map(|path| dir.join(path));
        for (key, secret) in &rule.secrets {
            if let Some(ResolvedEntry::Secret { key, .. }) = resolve_entry(key, secret, &saved_user)
                && let Ok((name, _)) = binding_name(&key)
            {
                output_for.insert(name, output.clone());
            }
        }
    }
    for (name, value) in &hit.values {
        let output = output_for.get(name).cloned().flatten();
        vars.entry(output)
            .or_default()
            .insert(name.clone(), value.clone());
    }
    let mut flat = HashMap::new();
    for values in vars.values() {
        flat.extend(values.clone());
    }
    crate::hub::store(
        cwd,
        config.walk_hash(),
        &saved_user,
        patterned,
        &flat,
        &hit.keys,
    );
    let cached = hit.keys;
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
                anyhow::bail!("conflicting binding '{key}' between secret and tunnel providers")
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
        cached,
        files,
        _network: network,
    })
}
