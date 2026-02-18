mod convert;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use access_json::JSONQuery;
use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use serde_json::Value;
use tokio::fs;
use url::Url;

use itertools::Itertools;

use super::Provider;
use crate::Hydration;
use convert::{ini2json, toml2json};

#[derive(Default)]
pub struct File {
    urls: HashMap<Url, String>,
}

impl File {
    pub fn new() -> Self {
        Default::default()
    }
}

#[async_trait]
impl Provider for File {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            Ok(url)
                if url.scheme() == "file"
                    && url.query_pairs().into_iter().any(|(k, _v)| k == "query") =>
            {
                self.urls.insert(url, value);
                Ok(())
            }
            _ => bail!("Not an file scheme or missing ?query=.field part"),
        }
    }

    async fn resolve(&self, cwd: &Path, _: &HashMap<String, String>) -> Result<Hydration> {
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|(raw_url, value)| {
                let url = value
                    .replace("file://", "")
                    .replace(&format!("?{}", raw_url.query().unwrap()), "");
                let user = directories::UserDirs::new().expect("cannot get HOME location");

                if url.starts_with("~/") {
                    user.home_dir()
                        .join(url.chars().skip(2).collect::<String>())
                } else if url.starts_with("$HOME/") {
                    user.home_dir()
                        .join(url.chars().skip(6).collect::<String>())
                } else {
                    cwd.join(url)
                }
            })
            .into_iter()
            .map(|(mut path, group)| async move {
                if !path.starts_with(PathBuf::from_str("/")?) {
                    path = cwd.join(path);
                }
                let format = path
                    .extension()
                    .expect("no file format found")
                    .to_str()
                    .expect("cannot get file format");
                let str = fs::read_to_string(&path)
                    .await
                    .unwrap_or_else(|_| panic!("cannot read file {}", path.display()));
                let json = match format {
                    "yaml" | "yml" => serde_yaml::from_str::<Value>(&str)?,
                    "json" => serde_json::from_str::<Value>(&str)?,
                    "toml" => toml2json(toml::from_str::<toml::Value>(&str)?),
                    "ini" => ini2json(ini::Ini::load_from_str(&str)?),
                    _ => bail!("unsupported file format: {}", format),
                };

                let hydration = group
                    .into_iter()
                    .map(|(url, value)| {
                        let query = url
                            .query_pairs()
                            .into_iter()
                            .find(|(k, _v)| k == "query")
                            .unwrap()
                            .1;
                        let compiled = JSONQuery::parse(&query)
                            .unwrap_or_else(|_| panic!("cannot compile query {}", query));
                        let res = compiled
                            .execute(&json)
                            .unwrap()
                            .unwrap_or_else(|| panic!("no query result for {}", query));
                        let output = match res {
                            Value::String(s) => s,
                            x => x.to_string(),
                        };
                        (value.clone(), output)
                    })
                    .collect::<Hydration>();

                Ok(hydration)
            })
            .collect::<Vec<_>>();

        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn test_add_routing() {
        let mut p = File::new();
        assert!(
            p.add("file:///path/to/config.json?query=.key".to_string())
                .is_ok()
        );
        assert!(p.add("file:///path/to/config.json".to_string()).is_err());
        assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    }

    async fn resolve_file(
        dir: &tempfile::TempDir,
        filename: &str,
        content: &str,
        query: &str,
    ) -> String {
        let path = dir.path().join(filename);
        std::fs::write(&path, content).unwrap();
        let url = format!("file://{}?query={}", path.display(), query);
        let mut p = File::new();
        p.add(url.clone()).unwrap();
        p.resolve(dir.path(), &HashMap::new())
            .await
            .unwrap()
            .remove(&url)
            .unwrap()
    }

    #[tokio::test]
    async fn test_resolve_json_file() {
        let dir = tempdir().unwrap();
        assert_eq!(
            resolve_file(&dir, "config.json", r#"{"key":"myvalue"}"#, ".key").await,
            "myvalue"
        );
    }

    #[tokio::test]
    async fn test_resolve_yaml_file() {
        let dir = tempdir().unwrap();
        assert_eq!(
            resolve_file(&dir, "config.yaml", "key: yamlvalue\n", ".key").await,
            "yamlvalue"
        );
    }

    #[tokio::test]
    async fn test_resolve_toml_file() {
        let dir = tempdir().unwrap();
        assert_eq!(
            resolve_file(&dir, "config.toml", "key = \"tomlvalue\"\n", ".key").await,
            "tomlvalue"
        );
    }

    #[tokio::test]
    async fn test_resolve_ini_file() {
        let dir = tempdir().unwrap();
        assert_eq!(
            resolve_file(
                &dir,
                "config.ini",
                "[section]\npassword = inivalue\n",
                ".section.password"
            )
            .await,
            "inivalue"
        );
    }

    #[tokio::test]
    async fn test_resolve_nested_json_query() {
        let dir = tempdir().unwrap();
        assert_eq!(
            resolve_file(
                &dir,
                "config.json",
                r#"{"db":{"password":"nested_pass"}}"#,
                ".db.password"
            )
            .await,
            "nested_pass"
        );
    }
}
