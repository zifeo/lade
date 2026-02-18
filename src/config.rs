use crate::global_config::GlobalConfig;
use anyhow::Result;
use futures::future::try_join_all;
use indexmap::IndexMap;
use lade_sdk::{hydrate, hydrate_one};
use regex::Regex;
use serde::Deserialize;
use std::fs::File;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub type Output = Option<PathBuf>;

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum LadeSecret {
    Secret(String),
    User(HashMap<String, Option<String>>),
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct RuleConfig {
    pub file: Option<PathBuf>,
    #[serde(rename = "1password_service_account")]
    pub onepassword_service_account: Option<LadeSecret>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LadeRule {
    #[serde(rename = ".")]
    pub config: Option<RuleConfig>,
    #[serde(flatten)]
    pub secrets: HashMap<String, LadeSecret>,
}

#[derive(Deserialize, Debug)]
pub struct LadeFile {
    #[serde(flatten)]
    pub commands: IndexMap<String, LadeRule>,
}

fn resolve_lade_secret(secret: &LadeSecret, user: &Option<String>) -> Option<String> {
    match secret {
        LadeSecret::Secret(value) => Some(value.clone()),
        LadeSecret::User(map) => user
            .as_ref()
            .and_then(|u| map.get(u))
            .or_else(|| map.get("."))
            .and_then(|v| v.clone()),
    }
}

impl LadeFile {
    pub fn from_path(path: &Path) -> Result<LadeFile> {
        let file = File::open(path).unwrap();
        let mut config: serde_yaml::Value = serde_yaml::from_reader(file)?;
        config.apply_merge()?;
        let config: LadeFile = serde_yaml::from_value(config)?;
        Ok(config)
    }

    pub fn build(path: PathBuf) -> Result<Config> {
        let mut configs: Vec<(PathBuf, LadeFile)> = Vec::default();
        let mut path = path;

        while {
            let yaml = path.join("lade.yaml");
            if yaml.exists() {
                configs.push((path.clone(), LadeFile::from_path(&yaml)?));
            } else {
                let yml = path.join("lade.yml");
                if yml.exists() {
                    configs.push((path.clone(), LadeFile::from_path(&yml)?));
                }
            }

            match path.parent() {
                Some(parent) => {
                    path = parent.to_path_buf();
                    true
                }
                None => false,
            }
        } {}

        let mut matches = Vec::default();

        configs.reverse();
        for (path, config) in configs.into_iter() {
            for (key, value) in config.commands.into_iter() {
                matches.push((Regex::new(&key)?, path.clone(), value));
            }
        }

        Ok(Config { matches })
    }
}

pub struct Config {
    matches: Vec<(Regex, PathBuf, LadeRule)>,
}

impl Config {
    fn collect(&self, command: &str) -> Vec<(PathBuf, LadeRule)> {
        self.matches
            .clone()
            .into_iter()
            .filter(|(regex, _, _)| regex.is_match(command))
            .map(|(_, path, env)| (path, env))
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

        let output = rule.config.as_ref().and_then(|c| c.file.clone());

        let extra_env = match rule
            .config
            .as_ref()
            .and_then(|c| c.onepassword_service_account.as_ref())
        {
            Some(sa_secret) => match resolve_lade_secret(sa_secret, &saved_user) {
                Some(uri) => {
                    let token = hydrate_one(uri, &path, &HashMap::new()).await?;
                    HashMap::from([("OP_SERVICE_ACCOUNT_TOKEN".to_string(), token)])
                }
                None => HashMap::new(),
            },
            None => HashMap::new(),
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
            |mut acc: HashMap<Option<PathBuf>, HashMap<String, String>>, (output, map)| {
                acc.entry(output).or_default().extend(map);
                acc
            },
        );
        Ok(ret)
    }

    pub fn collect_keys(&self, command: &str) -> HashMap<Output, Vec<String>> {
        self.collect(command)
            .into_iter()
            .map(|(_, env)| {
                (
                    env.config.as_ref().and_then(|c| c.file.clone()),
                    env.secrets.keys().cloned().collect::<Vec<_>>(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::*;
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_lade_secrets_on_yaml() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");

        let mut file = File::create(&file_path).unwrap();
        file.write_all(
            b"
                \"test command\":
                    \".\": { file: \"output/path\" }
                    secret1: \"secret_value\"
                    secret2:
                        user: \"user_name\"
                        password: \"password_value\"
                ",
        )
        .unwrap();

        let lade_file = LadeFile::from_path(&file_path).unwrap();

        let command = lade_file.commands.get("test command").unwrap();
        assert_eq!(
            command.config.as_ref().unwrap().file,
            Some(PathBuf::from("output/path"))
        );
        assert!(
            command
                .config
                .as_ref()
                .unwrap()
                .onepassword_service_account
                .is_none()
        );

        let secrets = &command.secrets;
        assert_eq!(secrets.len(), 2);

        let secret1 = secrets.get("secret1").unwrap();
        if let LadeSecret::Secret(value) = secret1 {
            assert_eq!(value, "secret_value");
        } else {
            panic!("secret1 should be a LadeSecret::Secret");
        }

        let secret2 = secrets.get("secret2").unwrap();
        if let LadeSecret::User(map) = secret2 {
            let mut expected = HashMap::new();
            expected.insert("user".to_string(), Some("user_name".to_string()));
            expected.insert("password".to_string(), Some("password_value".to_string()));
            assert_eq!(*map, expected);
        } else {
            panic!("secret2 should be a LadeSecret::User");
        }
    }

    #[test]
    fn test_rule_config_file_only() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"\"cmd\":\n  \".\": { file: \"out.yaml\" }\n  KEY: val\n")
            .unwrap();

        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let rule = lade_file.commands.get("cmd").unwrap();
        let config = rule.config.as_ref().unwrap();
        assert_eq!(config.file, Some(PathBuf::from("out.yaml")));
        assert!(config.onepassword_service_account.is_none());
    }

    #[test]
    fn test_rule_config_sa_string() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(
            b"\"cmd\":\n  \".\":\n    1password_service_account: \"op://host/vault/item\"\n  KEY: val\n",
        )
        .unwrap();

        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let rule = lade_file.commands.get("cmd").unwrap();
        let config = rule.config.as_ref().unwrap();
        assert!(config.file.is_none());
        assert!(matches!(
            config.onepassword_service_account.as_ref().unwrap(),
            LadeSecret::Secret(s) if s == "op://host/vault/item"
        ));
    }

    #[test]
    fn test_rule_config_sa_user_map_with_default() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(
            b"\"cmd\":\n  \".\":\n    1password_service_account:\n      zifeo: \"op://host/vault/item\"\n      \".\": null\n  KEY: val\n",
        )
        .unwrap();

        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let rule = lade_file.commands.get("cmd").unwrap();
        let config = rule.config.as_ref().unwrap();
        if let LadeSecret::User(map) = config.onepassword_service_account.as_ref().unwrap() {
            assert_eq!(
                map.get("zifeo"),
                Some(&Some("op://host/vault/item".to_string()))
            );
            assert_eq!(map.get("."), Some(&None));
        } else {
            panic!("expected LadeSecret::User");
        }
    }

    #[test]
    fn test_rule_config_absent() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"\"cmd\":\n  KEY: val\n").unwrap();

        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let rule = lade_file.commands.get("cmd").unwrap();
        assert!(rule.config.is_none());
    }

    #[test]
    fn test_old_format_dot_string_fails() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"\"cmd\":\n  \".\": \"some/path\"\n  KEY: val\n")
            .unwrap();

        assert!(LadeFile::from_path(&file_path).is_err());
    }

    #[test]
    fn test_resolve_lade_secret_string() {
        let secret = LadeSecret::Secret("value".to_string());
        assert_eq!(
            resolve_lade_secret(&secret, &Some("any".to_string())),
            Some("value".to_string())
        );
        assert_eq!(
            resolve_lade_secret(&secret, &None),
            Some("value".to_string())
        );
    }

    #[test]
    fn test_resolve_lade_secret_user_match() {
        let mut map = HashMap::new();
        map.insert("zifeo".to_string(), Some("secret_for_zifeo".to_string()));
        map.insert(".".to_string(), Some("default_secret".to_string()));
        let secret = LadeSecret::User(map);

        assert_eq!(
            resolve_lade_secret(&secret, &Some("zifeo".to_string())),
            Some("secret_for_zifeo".to_string())
        );
    }

    #[test]
    fn test_resolve_lade_secret_user_default_fallback() {
        let mut map = HashMap::new();
        map.insert("zifeo".to_string(), Some("secret_for_zifeo".to_string()));
        map.insert(".".to_string(), Some("default_secret".to_string()));
        let secret = LadeSecret::User(map);

        assert_eq!(
            resolve_lade_secret(&secret, &Some("other_user".to_string())),
            Some("default_secret".to_string())
        );
        assert_eq!(
            resolve_lade_secret(&secret, &None),
            Some("default_secret".to_string())
        );
    }

    #[test]
    fn test_resolve_lade_secret_user_no_match_no_default() {
        let mut map = HashMap::new();
        map.insert("zifeo".to_string(), Some("secret_for_zifeo".to_string()));
        let secret = LadeSecret::User(map);

        assert_eq!(
            resolve_lade_secret(&secret, &Some("other".to_string())),
            None
        );
        assert_eq!(resolve_lade_secret(&secret, &None), None);
    }

    #[test]
    fn test_resolve_lade_secret_user_null_default() {
        let mut map = HashMap::new();
        map.insert("zifeo".to_string(), Some("secret_for_zifeo".to_string()));
        map.insert(".".to_string(), None);
        let secret = LadeSecret::User(map);

        assert_eq!(
            resolve_lade_secret(&secret, &Some("other".to_string())),
            None
        );
        assert_eq!(resolve_lade_secret(&secret, &None), None);
    }
}
