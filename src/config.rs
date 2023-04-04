use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;
use futures::future::try_join_all;
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
    matches: Vec<(Regex, PathBuf, HashMap<String, String>)>,
}

impl Config {
    fn collect(&self, command: String) -> Vec<(PathBuf, HashMap<String, String>)> {
        self.matches
            .clone()
            .into_iter()
            .filter(|(regex, _, _)| regex.is_match(&command))
            .map(|(_, path, env)| (path, env))
            .collect()
    }

    pub async fn collect_hydrate(&self, command: String) -> Result<HashMap<String, String>> {
        let ret = try_join_all(
            self.collect(command)
                .into_iter()
                .map(|(path, env)| hydrate(env, path)),
        )
        .await?
        .into_iter()
        .fold(HashMap::default(), |mut acc, map| {
            acc.extend(map);
            acc
        });
        Ok(ret)
    }

    pub fn collect_keys(&self, command: String) -> Vec<String> {
        self.collect(command)
            .into_iter()
            .flat_map(|(_, env)| env.keys().cloned().collect::<Vec<_>>())
            .collect()
    }
}
