use anyhow::{Result, bail};
use log::debug;
use rustc_hash::FxHashSet;
use std::{
    collections::{BTreeMap, HashMap, HashSet, hash_map::Keys},
    ffi::OsStr,
    fs,
    io::{ErrorKind, Write},
    path::PathBuf,
    time::Instant,
};
use tokio::{signal, time};

use crate::config::{Config, Output, SecretSources};
use crate::network::{
    ProviderProgressEvent, ProviderProgressKind, format_elapsed_ms, format_timing,
};
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
    cached: &HashSet<String>,
) -> Result<LoadedSecrets> {
    let progress_groups = secret_progress_groups(plan, cached);
    let mut pending = HashMap::<String, PendingLine>::new();
    for group in &progress_groups {
        if group.fetched {
            let started = Instant::now();
            progress.send(ProviderProgressEvent {
                id: group.id.clone(),
                display: group.display.clone(),
                kind: ProviderProgressKind::Connecting,
            });
            pending.insert(
                group.id.clone(),
                PendingLine {
                    display: group.display.clone(),
                    started,
                    remaining: group.keys.clone(),
                },
            );
        } else {
            progress.send(ProviderProgressEvent {
                id: group.id.clone(),
                display: format_elapsed_ms(&group.display, 0),
                kind: ProviderProgressKind::Connected,
            });
        }
    }
    let sink = progress.clone();
    let hydrated = Config::hydrate_work_on_group(secrets, op_sa, |done| {
        for (id, display, started) in take_finished_lines(&mut pending, done) {
            sink.send(ProviderProgressEvent {
                id,
                display: format_timing(&display, started),
                kind: ProviderProgressKind::Connected,
            });
        }
    })
    .await;
    if let Err(e) = &hydrated {
        for (id, line) in pending.drain() {
            progress.send(ProviderProgressEvent {
                id,
                display: format_timing(&line.display, line.started),
                kind: ProviderProgressKind::Failed,
            });
        }
        return Err(anyhow::anyhow!(e.to_string()));
    }
    for (id, line) in pending.drain() {
        progress.send(ProviderProgressEvent {
            id,
            display: format_timing(&line.display, line.started),
            kind: ProviderProgressKind::Connected,
        });
    }
    let (vars, sources, maskable, warnings) = hydrated?;
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
            if crate::config::is_shell_uri(source) {
                return scheme.to_string();
            }
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

#[derive(Debug)]
struct SecretProgressGroup {
    id: String,
    display: String,
    fetched: bool,
    keys: HashSet<String>,
}

struct PendingLine {
    display: String,
    started: Instant,
    remaining: HashSet<String>,
}

fn take_finished_lines(
    pending: &mut HashMap<String, PendingLine>,
    done: &HashSet<String>,
) -> Vec<(String, String, Instant)> {
    let mut finished = Vec::new();
    pending.retain(|id, line| {
        for key in done {
            line.remaining.remove(key);
        }
        if line.remaining.is_empty() {
            finished.push((id.clone(), line.display.clone(), line.started));
            false
        } else {
            true
        }
    });
    finished
}

fn secret_progress_groups(
    plan: &SecretSources,
    cached: &HashSet<String>,
) -> Vec<SecretProgressGroup> {
    let mut groups = BTreeMap::<String, (Vec<String>, bool, HashSet<String>)>::new();
    for (key, source) in &plan.sources {
        if plan.silent.contains(key) || crate::family::is_raw_secret(source) {
            continue;
        }
        let from_hub = cached.contains(key);
        let name = progress_binding_name(key, plan.overridden.contains(key), from_hub);
        let entry = groups.entry(provider_label(source)).or_default();
        entry.0.push(name);
        if !from_hub {
            entry.1 = true;
            entry.2.insert(key.clone());
        }
    }
    for (key, source) in &plan.cancelled {
        if plan.silent.contains(key) || crate::family::is_raw_secret(source) {
            continue;
        }
        groups
            .entry(provider_label(source))
            .or_default()
            .0
            .push(format!("{key} (u)"));
    }
    groups
        .into_iter()
        .map(|(label, (mut names, fetched, keys))| {
            names.sort();
            SecretProgressGroup {
                id: format!("secret|{label}"),
                display: format!("{label}: {}", names.join(", ")),
                fetched,
                keys,
            }
        })
        .collect()
}

fn progress_binding_name(key: &str, overridden: bool, cached: bool) -> String {
    match (overridden, cached) {
        (true, true) => format!("{key} (o, c)"),
        (true, false) => format!("{key} (o)"),
        (false, true) => format!("{key} (c)"),
        (false, false) => key.to_string(),
    }
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
