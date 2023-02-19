use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;
use indexmap::IndexMap;
use lade_sdk::hydrate;
use regex::Regex;
use serde::Deserialize;
use std::fs::File;

#[derive(Deserialize, Debug)]
pub struct LadeFile {
    #[serde(flatten)]
    pub commands: IndexMap<String, HashMap<String, String>>,
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
        let mut configs: Vec<LadeFile> = Vec::default();
        let mut path = path;

        while {
            let config_path = path.join("lade.yaml");
            if config_path.exists() {
                configs.push(LadeFile::from_path(&config_path)?);
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
        for config in configs.into_iter() {
            for (key, value) in config.commands.into_iter() {
                matches.push((Regex::new(&key)?, value));
            }
        }

        Ok(Config { matches })
    }
}

pub struct Config {
    matches: Vec<(Regex, HashMap<String, String>)>,
}

impl Config {
    fn collect(&self, command: String) -> Result<HashMap<String, String>> {
        let mut ret: HashMap<String, String> = HashMap::default();
        for (regex, env) in self.matches.iter() {
            if regex.is_match(&command) {
                ret.extend(env.clone());
            }
        }
        Ok(ret)
    }

    pub async fn collect_hydrate(&self, command: String) -> Result<HashMap<String, String>> {
        hydrate(self.collect(command)?).await
    }

    pub fn collect_keys(&self, command: String) -> Result<Vec<String>> {
        Ok(self.collect(command)?.keys().cloned().collect())
    }
}
