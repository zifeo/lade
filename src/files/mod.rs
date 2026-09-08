use anyhow::{Result, bail};
use log::debug;
use rustc_hash::FxHashSet;
use std::{
    collections::{BTreeMap, HashMap, hash_map::Keys},
    ffi::OsStr,
    fs,
    io::{ErrorKind, Write},
    path::PathBuf,
    time::Instant,
};
use tokio::{signal, time};

use crate::config::{Config, Output, SecretSources};
use crate::network::{ProviderProgressEvent, ProviderProgressKind, format_timing};
use crate::provider_progress::ProviderProgressSink;
use crate::ticket::TicketSecret;

pub async fn sleep_or_cancel(secs: u64) {
    tokio::select! {
        _ = time::sleep(time::Duration::from_secs(secs)) => {},
        _ = signal::ctrl_c() => {
            std::process::exit(130);
        }
    }
}

pub struct LoadedSecrets {
    pub vars: HashMap<Output, HashMap<String, String>>,
    /// Env var name → config source (`lade.yml` value).
    pub sources: HashMap<String, String>,
    /// Config sources handled by providers that mask subprocess output.
    pub maskable: FxHashSet<String>,
    /// Warnings collected during resolution (e.g. provider fallbacks).
    pub warnings: Vec<String>,
}

/// Hydrates ticket secrets from a pre-event. `op_sa` is the resolved
/// 1Password service-account URI, not the token.
pub async fn hydrate_secrets_from_ticket_with_progress(
    secrets: &[TicketSecret],
    op_sa: Option<&str>,
    plan: &SecretSources,
    progress: ProviderProgressSink,
) -> Result<LoadedSecrets> {
    let started = Instant::now();
    let progress_groups = secret_progress_groups(plan);
    for (id, display) in &progress_groups {
        progress.send(ProviderProgressEvent {
            id: id.clone(),
            display: display.clone(),
            kind: ProviderProgressKind::Connecting,
        });
    }
    let hydrated = Config::hydrate_work(secrets, op_sa).await;
    if let Err(e) = &hydrated {
        for (id, display) in &progress_groups {
            progress.send(ProviderProgressEvent {
                id: id.clone(),
                display: format_timing(display, started),
                kind: ProviderProgressKind::Failed,
            });
        }
        return Err(anyhow::anyhow!(e.to_string()));
    }
    let (vars, sources, maskable, warnings) = hydrated?;
    for (id, display) in &progress_groups {
        progress.send(ProviderProgressEvent {
            id: id.clone(),
            display: format_timing(display, started),
            kind: ProviderProgressKind::Connected,
        });
    }
    Ok(LoadedSecrets {
        vars,
        sources,
        maskable,
        warnings,
    })
}

fn provider_label(source: &str) -> String {
    match source.split_once("://") {
        Some((scheme, rest)) => {
            let provider = rest.split('/').next().unwrap_or(rest);
            match scheme {
                "op" => format!("1Password {provider}"),
                "doppler" => format!("Doppler {provider}"),
                "infisical" => format!("Infisical {provider}"),
                "vault" => format!("Vault {provider}"),
                "passbolt" => format!("Passbolt {provider}"),
                "file" => "File".to_string(),
                "awssm" => format!("AWS Secrets Manager {provider}"),
                "azurekv" => format!("Azure Key Vault {provider}"),
                "bw" => format!("Bitwarden {provider}"),
                "gcpsm" => format!("GCP Secret Manager {provider}"),
                "age" => "age".to_string(),
                "sops" => "SOPS".to_string(),
                other => format!("{other} {provider}"),
            }
        }
        None => "Raw".to_string(),
    }
}

fn secret_progress_groups(plan: &SecretSources) -> Vec<(String, String)> {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for (key, source) in &plan.sources {
        if plan.silent.contains(key) {
            continue;
        }
        let name = if plan.overridden.contains(key) {
            format!("{key} (overridden)")
        } else {
            key.clone()
        };
        groups.entry(provider_label(source)).or_default().push(name);
    }
    for (key, source) in &plan.cancelled {
        if plan.silent.contains(key) {
            continue;
        }
        groups
            .entry(provider_label(source))
            .or_default()
            .push(format!("{key} (cancelled)"));
    }
    groups
        .into_iter()
        .map(|(label, mut keys)| {
            keys.sort();
            let display = format!("{label}: {}", keys.join(", "));
            (format!("secret|{label}"), display)
        })
        .collect()
}

pub fn write_files(hydration: &HashMap<PathBuf, HashMap<String, String>>) -> Result<Vec<String>> {
    let mut names = vec![];
    let mut files = Vec::new();
    for (path, vars) in hydration {
        names.extend(vars.keys().cloned());
        debug!("writing file: {:?}", path);
        let mut content: String = match path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or_else(|| panic!("cannot get extension of file: {:?}", path.display()))
        {
            "json" => serde_json::to_string(&vars)?,
            "yaml" | "yml" => serde_yaml::to_string(&vars)?,
            _ => bail!("unsupported file extension: {:?}", path.extension()),
        };
        if !content.ends_with('\n') {
            content.push('\n');
        }
        files.push((path, content));
    }

    let mut written = Vec::new();
    for (path, content) in files {
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(file) => file,
            Err(error) => {
                for path in written {
                    let _ = fs::remove_file(path);
                }
                if error.kind() == ErrorKind::AlreadyExists {
                    bail!("file already exists: {:?}", path);
                }
                return Err(error.into());
            }
        };
        if let Err(error) = file.write_all(content.as_bytes()) {
            let _ = fs::remove_file(path);
            for path in written {
                let _ = fs::remove_file(path);
            }
            return Err(error.into());
        }
        written.push(path);
    }
    Ok(names)
}

pub fn remove_files<T>(files: &mut Keys<PathBuf, T>) -> Result<()> {
    for path in files {
        debug!("removing file: {:?}", path);
        if let Err(error) = fs::remove_file(path)
            && error.kind() != ErrorKind::NotFound
        {
            return Err(error.into());
        }
    }
    Ok(())
}
pub fn split_env_files<T: Default>(mut hydration: HashMap<Output, T>) -> (T, HashMap<PathBuf, T>) {
    let env = hydration.remove(&None).unwrap_or_default();
    let files = hydration
        .into_iter()
        .filter_map(|(path, vars)| path.map(|p| (p, vars)))
        .collect();
    (env, files)
}

#[cfg(test)]
mod tests;
