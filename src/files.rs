use anyhow::{Result, bail};
use log::debug;
use rustc_hash::FxHashSet;
use std::{
    collections::{BTreeMap, HashMap, hash_map::Keys},
    ffi::OsStr,
    fs,
    path::PathBuf,
    time::Instant,
};
use tokio::{signal, time};

use crate::config::{Config, LadeRule, Output};
use crate::network::{ProviderProgressEvent, ProviderProgressKind, format_timing};
use crate::provider_progress::ProviderProgressSink;

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

/// Hydrates already-collected `rules` against an already-resolved
/// `saved_user`. Callers should resolve both once per invocation (see
/// [`Config::collect`]/[`crate::config::saved_user`]) and reuse them here
/// instead of letting this re-match `command` and re-read the global config.
pub async fn hydrate_secrets_with_progress(
    config: &Config,
    rules: &[(PathBuf, LadeRule)],
    saved_user: &Option<String>,
    progress: ProviderProgressSink,
) -> Result<LoadedSecrets> {
    let started = Instant::now();
    let secret_sources = Config::secret_sources_from_rules(rules, saved_user)?;
    let progress_groups = secret_progress_groups(&secret_sources);
    for (id, display) in &progress_groups {
        progress.send(ProviderProgressEvent {
            id: id.clone(),
            display: display.clone(),
            kind: ProviderProgressKind::Connecting,
        });
    }
    let hydrated = config.hydrate_rules(rules, saved_user).await;
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

fn secret_progress_groups(sources: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for (key, source) in sources {
        let label = match source.split_once("://") {
            Some((scheme, rest)) => {
                let provider = rest.split('/').next().unwrap_or(rest);
                match scheme {
                    "op" => format!("1Password {provider}"),
                    "doppler" => format!("Doppler {provider}"),
                    "infisical" => format!("Infisical {provider}"),
                    "vault" => format!("Vault {provider}"),
                    "passbolt" => format!("Passbolt {provider}"),
                    "file" => "File".to_string(),
                    other => format!("{other} {provider}"),
                }
            }
            None => "Raw".to_string(),
        };
        groups.entry(label).or_default().push(key.clone());
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
    for (path, vars) in hydration {
        names.extend(vars.keys().cloned());
        if path.exists() {
            bail!("file already exists: {:?}", path)
        }
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
        fs::write(path, content)?;
    }
    Ok(names)
}

pub fn remove_files<T>(files: &mut Keys<PathBuf, T>) -> Result<()> {
    for path in files {
        debug!("removing file: {:?}", path);
        if !path.exists() {
            bail!("file should have existed: {:?}", path)
        }
        fs::remove_file(path)?;
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
mod tests {
    use super::*;
    use std::{collections::HashMap, path::PathBuf};
    use tempfile::tempdir;

    #[test]
    fn test_split_env_only() {
        let hydration: HashMap<Output, HashMap<String, String>> = HashMap::from([(
            None,
            HashMap::from([("KEY".to_string(), "val".to_string())]),
        )]);
        let (env, files) = split_env_files(hydration);
        assert_eq!(env.get("KEY").unwrap(), "val");
        assert!(files.is_empty());
    }

    #[test]
    fn test_split_files_only() {
        let path = PathBuf::from("/tmp/secrets_lade_test.json");
        let hydration: HashMap<Output, HashMap<String, String>> = HashMap::from([(
            Some(path.clone()),
            HashMap::from([("KEY".to_string(), "val".to_string())]),
        )]);
        let (env, files) = split_env_files(hydration);
        assert!(env.is_empty());
        assert_eq!(files.get(&path).unwrap().get("KEY").unwrap(), "val");
    }

    #[test]
    fn test_split_mixed() {
        let path = PathBuf::from("/tmp/secrets_lade_mixed.json");
        let hydration: HashMap<Output, HashMap<String, String>> = HashMap::from([
            (
                None,
                HashMap::from([("ENV_KEY".to_string(), "env_val".to_string())]),
            ),
            (
                Some(path.clone()),
                HashMap::from([("FILE_KEY".to_string(), "file_val".to_string())]),
            ),
        ]);
        let (env, files) = split_env_files(hydration);
        assert_eq!(env.get("ENV_KEY").unwrap(), "env_val");
        assert_eq!(
            files.get(&path).unwrap().get("FILE_KEY").unwrap(),
            "file_val"
        );
    }

    #[test]
    fn test_write_files_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output.json");
        let hydration = HashMap::from([(
            path.clone(),
            HashMap::from([("KEY".to_string(), "value".to_string())]),
        )]);
        let names = write_files(&hydration).unwrap();
        assert!(names.contains(&"KEY".to_string()));
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["KEY"].as_str().unwrap(), "value");
    }

    #[test]
    fn test_write_files_yaml() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output.yaml");
        let hydration = HashMap::from([(
            path.clone(),
            HashMap::from([("KEY".to_string(), "value".to_string())]),
        )]);
        write_files(&hydration).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("KEY") && content.contains("value"));
    }

    #[test]
    fn test_write_files_already_exists_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output.json");
        std::fs::write(&path, "{}").unwrap();
        let hydration = HashMap::from([(
            path.clone(),
            HashMap::from([("KEY".to_string(), "value".to_string())]),
        )]);
        assert!(write_files(&hydration).is_err());
    }

    #[test]
    fn test_write_files_unsupported_extension_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output.txt");
        let hydration = HashMap::from([(
            path.clone(),
            HashMap::from([("KEY".to_string(), "value".to_string())]),
        )]);
        assert!(write_files(&hydration).is_err());
    }

    #[test]
    fn test_remove_files_existing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.json");
        std::fs::write(&path, "{}").unwrap();
        let files: HashMap<PathBuf, HashMap<String, String>> =
            HashMap::from([(path.clone(), HashMap::new())]);
        remove_files(&mut files.keys()).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_remove_files_missing_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent_lade_test.json");
        let files: HashMap<PathBuf, HashMap<String, String>> =
            HashMap::from([(path, HashMap::new())]);
        assert!(remove_files(&mut files.keys()).is_err());
    }
}
