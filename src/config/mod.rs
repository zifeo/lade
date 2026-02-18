mod loader;
mod secret;

pub use loader::LadeFile;
use secret::resolve_lade_secret;
pub use secret::*;

use crate::global_config::GlobalConfig;
use anyhow::Result;
use futures::future::try_join_all;
use lade_sdk::{hydrate, hydrate_one};
use regex::Regex;
use std::{collections::HashMap, path::PathBuf};

pub type Output = Option<PathBuf>;

pub struct Config {
    matches: Vec<(Regex, PathBuf, LadeRule)>,
}

impl Config {
    pub(crate) fn new(matches: Vec<(Regex, PathBuf, LadeRule)>) -> Self {
        Config { matches }
    }

    pub(crate) fn collect(&self, command: &str) -> Vec<(PathBuf, LadeRule)> {
        self.matches
            .iter()
            .filter(|(regex, _, _)| regex.is_match(command))
            .map(|(_, path, rule)| (path.clone(), rule.clone()))
            .collect()
    }

    async fn hydrate_output(
        &self,
        path: PathBuf,
        rule: LadeRule,
    ) -> Result<(Output, HashMap<String, String>)> {
        use std::env;
        let local_config = GlobalConfig::load().await?;
        let saved_user = local_config
            .user
            .or_else(|| env::var("USER").ok().or_else(|| env::var("USERNAME").ok()));

        let secrets_with_single_user: HashMap<String, String> = rule
            .secrets
            .iter()
            .filter_map(|(key, secret)| {
                resolve_lade_secret(secret, &saved_user).map(|v| (key.clone(), v))
            })
            .collect();

        let config = rule.config.as_ref();
        let output = config.and_then(|c| c.file.clone());
        let extra_env = if let Some(uri) = config
            .and_then(|c| c.onepassword_service_account.as_ref())
            .and_then(|sa| resolve_lade_secret(sa, &saved_user))
        {
            let token = hydrate_one(uri, &path, &HashMap::new()).await?;
            HashMap::from([("OP_SERVICE_ACCOUNT_TOKEN".to_string(), token)])
        } else {
            HashMap::new()
        };

        hydrate(secrets_with_single_user, path.clone(), extra_env)
            .await
            .map(|h| (output.map(|subpath| path.join(subpath)), h))
    }

    pub async fn collect_hydrate(
        &self,
        command: &str,
    ) -> Result<HashMap<Output, HashMap<String, String>>> {
        let ret = try_join_all(
            self.collect(command)
                .into_iter()
                .map(|(path, rule)| self.hydrate_output(path, rule)),
        )
        .await?
        .into_iter()
        .fold(
            HashMap::default(),
            |mut acc: HashMap<Output, HashMap<String, String>>, (output, map)| {
                acc.entry(output).or_default().extend(map);
                acc
            },
        );
        Ok(ret)
    }

    pub fn collect_keys(&self, command: &str) -> HashMap<Output, Vec<String>> {
        self.collect(command)
            .into_iter()
            .map(|(_, rule)| {
                (
                    rule.config.as_ref().and_then(|c| c.file.clone()),
                    rule.secrets.keys().cloned().collect::<Vec<_>>(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::*;
    use tempfile::tempdir;

    #[test]
    fn test_collect_exact_match() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"terraform plan\":\n  KEY: val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect("terraform plan").len(), 1);
    }

    #[test]
    fn test_collect_regex_match() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"terraform.*\":\n  KEY: val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect("terraform plan").len(), 1);
        assert_eq!(config.collect("terraform apply").len(), 1);
        assert_eq!(config.collect("other command").len(), 0);
    }

    #[test]
    fn test_collect_no_match() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lade.yml"), "\"specific\":\n  KEY: val\n").unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert!(config.collect("other").is_empty());
    }

    #[test]
    fn test_collect_multiple_rules_match() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd.*\":\n  KEY1: val1\n\".*\":\n  KEY2: val2\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect("cmd anything").len(), 2);
    }

    #[test]
    fn test_collect_keys_env_output() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd\":\n  KEY1: val1\n  KEY2: val2\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let keys = config.collect_keys("cmd");
        let env_keys = keys.get(&None).unwrap();
        assert!(env_keys.contains(&"KEY1".to_string()));
        assert!(env_keys.contains(&"KEY2".to_string()));
    }

    #[test]
    fn test_collect_keys_file_output() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd\":\n  \".\": { file: \"secrets.json\" }\n  KEY: val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let keys = config.collect_keys("cmd");
        let file_entries: Vec<_> = keys.into_iter().filter(|(k, _)| k.is_some()).collect();
        assert_eq!(file_entries.len(), 1);
        assert!(file_entries[0].1.contains(&"KEY".to_string()));
    }

    #[test]
    fn test_collect_keys_no_match_empty() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lade.yml"), "\"cmd\":\n  KEY: val\n").unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert!(config.collect_keys("other").is_empty());
    }
}
