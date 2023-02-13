use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;
use regex::Regex;
use serde::Deserialize;

use std::fs::File;

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(flatten)]
    pub commands: HashMap<String, HashMap<String, String>>,
}

impl Config {
    pub fn from_path(path: &Path) -> Result<Config> {
        let file = File::open(&path).unwrap();
        let config = serde_yaml::from_reader(file)?;
        Ok(config)
    }

    pub fn build_envs(path: PathBuf) -> Result<Vec<(Regex, HashMap<String, String>)>> {
        let mut configs: Vec<Config> = Vec::default();
        let mut path = path;

        while {
            let config_path = path.join("lade.yaml");
            if config_path.exists() {
                configs.push(Config::from_path(&config_path)?);
            }

            match path.parent() {
                Some(parent) => {
                    path = parent.to_path_buf();
                    true
                }
                None => false,
            }
        } {}

        let mut ret: Vec<(Regex, HashMap<String, String>)> = Vec::default();

        configs.reverse();
        for config in configs.into_iter() {
            for (key, value) in config.commands.into_iter() {
                ret.push((Regex::new(&key)?, value));
            }
        }

        Ok(ret)
    }
}
