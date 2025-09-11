use crate::global_config::GlobalConfig;
use anyhow::Result;
use futures::future::try_join_all;
use indexmap::IndexMap;
use lade_sdk::hydrate;
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
    User(HashMap<String, String>),
}

#[derive(Deserialize, Debug, Clone)]
pub struct LadeRule {
    #[serde(rename = ".")]
    pub output: Output,
    #[serde(flatten)]
    pub secrets: HashMap<String, LadeSecret>,
}

#[derive(Deserialize, Debug)]
pub struct LadeFile {
    #[serde(flatten)]
    pub commands: IndexMap<String, LadeRule>,
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
        let saved_user = local_config.user;

        let secrets_with_single_user = rule.secrets.iter().fold(
            HashMap::new(),
            |mut acc, (key, secret)| {
                match secret {
                    LadeSecret::Secret(value) => {
                        acc.insert(key.to_string(), value.to_string());
                    }
                    LadeSecret::User(user_secrets) => {
                        let user = match saved_user.clone() {
                            Some(saved_user) => Some(saved_user),
                            None => {  // fallback the os user
                                if let Ok(unix_user) = env::var("USER") {
                                    Some(unix_user)
                                } else {
                                    if let Ok(windows_user) = env::var("USERNAME") {
                                        Some(windows_user)
                                    } else {
                                        None
                                    }
                                }

                            }
                        };
                        
                        match user {
                            Some(user) => {
                                if let Some(user_secret) = user_secrets.get(&user) {
                                    acc.insert(key.to_string(), user_secret.to_string());
                                } else {
                                    eprintln!("Error: No secret found for user '{}' and key '{}'. Set a user with 'lade set-user <USER>'.", user, key);
                                }
                            }
                            None => {
                                eprintln!("Error: Secret '{}' requires a user. Set one with 'lade set-user <USER>'.", key);
                            }
                        }
                    }
                }
                acc
            },
        );
        hydrate(secrets_with_single_user, path.clone())
            .await
            .map(|x| (rule.output.map(|subpath| path.clone().join(subpath)), x))
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
            .map(|(_, env)| (env.output, env.secrets.keys().cloned().collect::<Vec<_>>()))
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
        // Create a temporary directory
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");

        // Create a lade.yml file with test data
        let mut file = File::create(&file_path).unwrap();
        file.write_all(
            b"
                \"test command\":
                    \".\": \"output/path\"
                    secret1: \"secret_value\"
                    secret2:
                        user: \"user_name\"
                        password: \"password_value\"
                ",
        )
        .unwrap();

        println!("File path: {}", file_path.display());
        // Parse the lade.yml file
        let lade_file = LadeFile::from_path(&file_path).unwrap();

        // Assert that the parsed data is correct
        let command = lade_file.commands.get("test command").unwrap();
        assert_eq!(command.output, Some(PathBuf::from("output/path")));

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
            expected.insert("user".to_string(), "user_name".to_string());
            expected.insert("password".to_string(), "password_value".to_string());
            assert_eq!(*map, expected);
        } else {
            panic!("secret2 should be a LadeSecret::User");
        }
    }
}
