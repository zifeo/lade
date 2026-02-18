use anyhow::{Result, bail};
use log::debug;
use std::{
    collections::{HashMap, hash_map::Keys},
    ffi::OsStr,
    fs,
    path::PathBuf,
};
use tokio::time;

use crate::config::{Config, Output};

pub async fn hydration_or_exit(
    config: &Config,
    command: &str,
) -> HashMap<Output, HashMap<String, String>> {
    match config.collect_hydrate(command).await {
        Ok(hydration) => hydration,
        Err(e) => {
            let width = 80;
            let wrap_width = width - 4;
            let header = "Lade could not get secrets from one loader:";
            let error = e.to_string();
            let hint = "Hint: check whether the loader is connected? to the correct? vault.";
            let wait = "Waiting 5 seconds before continuing...";

            eprintln!("┌{}┐", "-".repeat(width - 2));
            eprintln!("| {} {}|", header, " ".repeat(wrap_width - header.len()));
            for line in textwrap::wrap(error.trim(), wrap_width - 2) {
                eprintln!(
                    "| > {} {}|",
                    line,
                    " ".repeat(wrap_width - 2 - textwrap::core::display_width(&line)),
                );
            }
            eprintln!("| {} {}|", hint, " ".repeat(wrap_width - hint.len()));
            eprintln!("| {} {}|", wait, " ".repeat(wrap_width - wait.len()));
            eprintln!("└{}┘", "-".repeat(width - 2));
            time::sleep(time::Duration::from_secs(5)).await;
            std::process::exit(1);
        }
    }
}

pub fn write_files(hydration: &HashMap<PathBuf, HashMap<String, String>>) -> Result<Vec<String>> {
    let mut names = vec![];
    for (path, vars) in hydration {
        names.extend(vars.keys().cloned());
        if path.exists() {
            bail!("file already exists: {:?}", path)
        }
        debug!("writing file: {:?}", path);
        let content: String = match path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or_else(|| panic!("cannot get extension of file: {:?}", path.display()))
        {
            "json" => serde_json::to_string(&vars)?,
            "yaml" | "yml" => serde_yaml::to_string(&vars)?,
            _ => bail!("unsupported file extension: {:?}", path.extension()),
        };
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
pub fn split_env_files<T: Default + ToOwned>(
    hydration: &mut HashMap<Output, T>,
) -> (T, HashMap<PathBuf, T>)
where
    HashMap<PathBuf, T>: FromIterator<(PathBuf, <T as ToOwned>::Owned)>,
{
    let env = hydration.remove(&None::<PathBuf>).unwrap_or_default();
    let files = hydration
        .iter_mut()
        .filter(|(path, _)| path.is_some())
        .map(|(path, vars)| (path.to_owned().unwrap(), vars.to_owned()))
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
        let mut hydration: HashMap<Output, HashMap<String, String>> = HashMap::from([(
            None,
            HashMap::from([("KEY".to_string(), "val".to_string())]),
        )]);
        let (env, files) = split_env_files(&mut hydration);
        assert_eq!(env.get("KEY").unwrap(), "val");
        assert!(files.is_empty());
    }

    #[test]
    fn test_split_files_only() {
        let path = PathBuf::from("/tmp/secrets_lade_test.json");
        let mut hydration: HashMap<Output, HashMap<String, String>> = HashMap::from([(
            Some(path.clone()),
            HashMap::from([("KEY".to_string(), "val".to_string())]),
        )]);
        let (env, files) = split_env_files(&mut hydration);
        assert!(env.is_empty());
        assert_eq!(files.get(&path).unwrap().get("KEY").unwrap(), "val");
    }

    #[test]
    fn test_split_mixed() {
        let path = PathBuf::from("/tmp/secrets_lade_mixed.json");
        let mut hydration: HashMap<Output, HashMap<String, String>> = HashMap::from([
            (
                None,
                HashMap::from([("ENV_KEY".to_string(), "env_val".to_string())]),
            ),
            (
                Some(path.clone()),
                HashMap::from([("FILE_KEY".to_string(), "file_val".to_string())]),
            ),
        ]);
        let (env, files) = split_env_files(&mut hydration);
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
