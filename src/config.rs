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

pub type Output = Option<PathBuf>;

#[derive(Deserialize, Debug, Clone)]
pub struct LadeRule {
    #[serde(rename = ".")]
    pub output: Output,
    #[serde(flatten)]
    pub secrets: HashMap<String, String>,
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
    fn collect(&self, command: String) -> Vec<(PathBuf, LadeRule)> {
        self.matches
            .clone()
            .into_iter()
            .filter(|(regex, _, _)| regex.is_match(&command))
            .map(|(_, path, env)| (path, env))
            .collect()
    }

    async fn hydrate_output(
        &self,
        path: PathBuf,
        rule: LadeRule,
    ) -> Result<(Output, HashMap<String, String>)> {
        hydrate(rule.secrets, path).await.map(|x| (rule.output, x))
    }

    pub async fn collect_hydrate(
        &self,
        command: String,
    ) -> Result<HashMap<Output, HashMap<String, String>>> {
        let ret = try_join_all(
            self.collect(command)
                .into_iter()
                .map(|(path, rule)| self.hydrate_output(path, rule)),
        )
        .await?
        .into_iter()
        .fold(HashMap::default(), |mut acc, (output, map)| {
            acc.entry(output)
                .or_insert_with(HashMap::default)
                .extend(map);
            acc
        });
        Ok(ret)
    }

    pub fn collect_keys(&self, command: String) -> HashMap<Output, Vec<String>> {
        self.collect(command)
            .into_iter()
            .map(|(_, env)| (env.output, env.secrets.keys().cloned().collect::<Vec<_>>()))
            .collect()
    }
}
