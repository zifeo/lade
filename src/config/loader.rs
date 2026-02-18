use anyhow::Result;
use indexmap::IndexMap;
use regex::Regex;
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

use super::{Config, secret::LadeRule};

#[derive(Deserialize, Debug)]
pub struct LadeFile {
    #[serde(flatten)]
    pub commands: IndexMap<String, LadeRule>,
}

impl LadeFile {
    pub fn from_path(path: &Path) -> Result<LadeFile> {
        let file = File::open(path)?;
        let mut config: serde_yaml::Value = serde_yaml::from_reader(file)?;
        config.apply_merge()?;
        Ok(serde_yaml::from_value(config)?)
    }

    pub fn build(mut path: PathBuf) -> Result<Config> {
        let mut configs: Vec<(PathBuf, LadeFile)> = Vec::default();

        loop {
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
                Some(parent) => path = parent.to_path_buf(),
                None => break,
            }
        }

        let mut matches = Vec::default();
        configs.reverse();
        for (path, config) in configs.into_iter() {
            for (key, value) in config.commands.into_iter() {
                matches.push((Regex::new(&key)?, path.clone(), value));
            }
        }

        Ok(Config::new(matches))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LadeSecret;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_rule_config_file_only() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            b"\"cmd\":\n  \".\": { file: \"out.yaml\" }\n  KEY: val\n",
        )
        .unwrap();
        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let rule = lade_file.commands.get("cmd").unwrap();
        let config = rule.config.as_ref().unwrap();
        assert_eq!(config.file, Some(PathBuf::from("out.yaml")));
        assert!(config.onepassword_service_account.is_none());
    }

    #[test]
    fn test_rule_config_absent() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(&file_path, b"\"cmd\":\n  KEY: val\n").unwrap();
        let lade_file = LadeFile::from_path(&file_path).unwrap();
        assert!(lade_file.commands.get("cmd").unwrap().config.is_none());
    }

    #[test]
    fn test_old_format_dot_string_fails() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            b"\"cmd\":\n  \".\": \"some/path\"\n  KEY: val\n",
        )
        .unwrap();
        assert!(LadeFile::from_path(&file_path).is_err());
    }

    #[test]
    fn test_multiple_commands_in_yaml() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            "\"cmd1\":\n  KEY1: val1\n\"cmd2\":\n  KEY2: val2\n",
        )
        .unwrap();
        let lade_file = LadeFile::from_path(&file_path).unwrap();
        assert_eq!(lade_file.commands.len(), 2);
        assert!(lade_file.commands.contains_key("cmd1"));
        assert!(lade_file.commands.contains_key("cmd2"));
    }

    #[test]
    fn test_build_single_lade_yml() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lade.yml"), "\"cmd\":\n  KEY: val\n").unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect("cmd").len(), 1);
    }

    #[test]
    fn test_build_yaml_extension_fallback() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lade.yaml"), "\"cmd\":\n  KEY: val\n").unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect("cmd").len(), 1);
    }

    #[test]
    fn test_build_yaml_preferred_over_yml() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yaml"),
            "\"cmd\":\n  KEY_YAML: yaml_val\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd\":\n  KEY_YML: yml_val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let matches = config.collect("cmd");
        assert_eq!(matches.len(), 1);
        assert!(matches[0].1.secrets.contains_key("KEY_YAML"));
    }

    #[test]
    fn test_build_nested_dirs_parent_first() {
        let parent = tempdir().unwrap();
        let child = parent.path().join("child");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(
            parent.path().join("lade.yml"),
            "\"cmd\":\n  PARENT_KEY: pval\n",
        )
        .unwrap();
        std::fs::write(child.join("lade.yml"), "\"cmd\":\n  CHILD_KEY: cval\n").unwrap();
        let config = LadeFile::build(child).unwrap();
        let matches = config.collect("cmd");
        assert_eq!(matches.len(), 2);
        assert!(matches[0].1.secrets.contains_key("PARENT_KEY"));
        assert!(matches[1].1.secrets.contains_key("CHILD_KEY"));
    }

    #[test]
    fn test_build_no_config_empty() {
        let dir = tempdir().unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert!(config.collect("anything").is_empty());
    }

    #[test]
    fn test_build_invalid_regex_error() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"[invalid regex\":\n  KEY: val\n",
        )
        .unwrap();
        assert!(LadeFile::build(dir.path().to_path_buf()).is_err());
    }
}
