use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use access_json::JSONQuery;
use anyhow::{Ok, Result, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use ini::Ini;
use itertools::Itertools;
use serde_json::Value;
use tokio::fs;
use url::Url;

use super::Provider;
use crate::Hydration;

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
            std::result::Result::Ok(url)
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

                let path = if url.starts_with("~/") {
                    user.home_dir()
                        .join(url.chars().skip(2).collect::<String>())
                } else if url.starts_with("$HOME/") {
                    user.home_dir()
                        .join(url.chars().skip(6).collect::<String>())
                } else {
                    cwd.join(url)
                };

                path
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
                    "toml" => {
                        let values = toml::from_str::<toml::Value>(&str)?;
                        toml2json(values)
                    }
                    "ini" => {
                        let values = Ini::load_from_str(&str)?;
                        ini2json(values)
                    }
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

fn ini2json(ini: Ini) -> Value {
    let mut map = HashMap::new();
    for (section, properties) in ini.iter() {
        let mut section_map = HashMap::new();
        for (key, value) in properties.iter() {
            section_map.insert(key.to_string(), Value::String(value.to_string()));
        }
        match section {
            Some(section) => {
                map.insert(
                    section.to_string(),
                    Value::Object(section_map.into_iter().collect()),
                );
            }
            None => {
                map.extend(section_map);
            }
        }
    }
    Value::Object(map.into_iter().collect())
}

fn toml2json(toml: toml::Value) -> Value {
    match toml {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => {
            Value::Number(serde_json::Number::from_f64(f).expect("nan not allowed"))
        }
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Array(arr) => Value::Array(arr.into_iter().map(toml2json).collect()),
        toml::Value::Table(table) => {
            Value::Object(table.into_iter().map(|(k, v)| (k, toml2json(v))).collect())
        }
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::tempdir;

    // --- add() routing ---

    #[test]
    fn test_add_valid_file_with_query() {
        let mut p = File::new();
        assert!(
            p.add("file:///path/to/config.json?query=.key".to_string())
                .is_ok()
        );
    }

    #[test]
    fn test_add_rejects_file_without_query() {
        let mut p = File::new();
        assert!(p.add("file:///path/to/config.json".to_string()).is_err());
    }

    #[test]
    fn test_add_rejects_non_file_scheme() {
        let mut p = File::new();
        assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    }

    // --- toml2json ---

    #[test]
    fn test_toml2json_string() {
        assert_eq!(
            toml2json(toml::Value::String("hello".to_string())),
            Value::String("hello".to_string())
        );
    }

    #[test]
    fn test_toml2json_integer() {
        assert_eq!(
            toml2json(toml::Value::Integer(42)),
            Value::Number(42.into())
        );
    }

    #[test]
    fn test_toml2json_boolean() {
        assert_eq!(toml2json(toml::Value::Boolean(true)), Value::Bool(true));
    }

    #[test]
    fn test_toml2json_array() {
        let arr = toml::Value::Array(vec![
            toml::Value::String("a".into()),
            toml::Value::String("b".into()),
        ]);
        let result = toml2json(arr);
        assert_eq!(
            result,
            Value::Array(vec![Value::String("a".into()), Value::String("b".into())])
        );
    }

    #[test]
    fn test_toml2json_nested_table() {
        let mut inner = toml::map::Map::new();
        inner.insert(
            "nested_key".to_string(),
            toml::Value::String("nested_val".to_string()),
        );
        let mut outer = toml::map::Map::new();
        outer.insert("section".to_string(), toml::Value::Table(inner));
        let result = toml2json(toml::Value::Table(outer));
        if let Value::Object(map) = result {
            if let Some(Value::Object(section)) = map.get("section") {
                assert_eq!(
                    section.get("nested_key").unwrap(),
                    &Value::String("nested_val".to_string())
                );
            } else {
                panic!("expected section Object");
            }
        } else {
            panic!("expected Object");
        }
    }

    // --- ini2json ---

    #[test]
    fn test_ini2json_section_with_properties() {
        let ini = Ini::load_from_str("[section1]\nkey1 = val1\n").unwrap();
        let result = ini2json(ini);
        if let Value::Object(map) = result {
            if let Some(Value::Object(section)) = map.get("section1") {
                assert_eq!(
                    section.get("key1").unwrap(),
                    &Value::String("val1".to_string())
                );
            } else {
                panic!("expected section1 Object");
            }
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn test_ini2json_global_key() {
        let ini = Ini::load_from_str("global_key = global_val\n").unwrap();
        let result = ini2json(ini);
        if let Value::Object(map) = result {
            assert_eq!(
                map.get("global_key").unwrap(),
                &Value::String("global_val".to_string())
            );
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn test_ini2json_multiple_sections() {
        let ini = Ini::load_from_str("[s1]\nk1 = v1\n\n[s2]\nk2 = v2\n").unwrap();
        let result = ini2json(ini);
        if let Value::Object(map) = result {
            assert!(map.contains_key("s1"));
            assert!(map.contains_key("s2"));
        } else {
            panic!("expected Object");
        }
    }

    // --- File provider resolve() ---

    #[tokio::test]
    async fn test_resolve_json_file() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("config.json");
        std::fs::write(&json_path, r#"{"key":"myvalue"}"#).unwrap();
        let url = format!("file://{}?query=.key", json_path.display());

        let mut p = File::new();
        p.add(url.clone()).unwrap();
        let result = p.resolve(dir.path(), &HashMap::new()).await.unwrap();
        assert_eq!(result.get(&url).unwrap(), "myvalue");
    }

    #[tokio::test]
    async fn test_resolve_yaml_file() {
        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("config.yaml");
        std::fs::write(&yaml_path, "key: yamlvalue\n").unwrap();
        let url = format!("file://{}?query=.key", yaml_path.display());

        let mut p = File::new();
        p.add(url.clone()).unwrap();
        let result = p.resolve(dir.path(), &HashMap::new()).await.unwrap();
        assert_eq!(result.get(&url).unwrap(), "yamlvalue");
    }

    #[tokio::test]
    async fn test_resolve_toml_file() {
        let dir = tempdir().unwrap();
        let toml_path = dir.path().join("config.toml");
        std::fs::write(&toml_path, "key = \"tomlvalue\"\n").unwrap();
        let url = format!("file://{}?query=.key", toml_path.display());

        let mut p = File::new();
        p.add(url.clone()).unwrap();
        let result = p.resolve(dir.path(), &HashMap::new()).await.unwrap();
        assert_eq!(result.get(&url).unwrap(), "tomlvalue");
    }

    #[tokio::test]
    async fn test_resolve_ini_file() {
        let dir = tempdir().unwrap();
        let ini_path = dir.path().join("config.ini");
        std::fs::write(&ini_path, "[section]\npassword = inivalue\n").unwrap();
        let url = format!("file://{}?query=.section.password", ini_path.display());

        let mut p = File::new();
        p.add(url.clone()).unwrap();
        let result = p.resolve(dir.path(), &HashMap::new()).await.unwrap();
        assert_eq!(result.get(&url).unwrap(), "inivalue");
    }

    #[tokio::test]
    async fn test_resolve_nested_json_query() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("config.json");
        std::fs::write(&json_path, r#"{"db":{"password":"nested_pass"}}"#).unwrap();
        let url = format!("file://{}?query=.db.password", json_path.display());

        let mut p = File::new();
        p.add(url.clone()).unwrap();
        let result = p.resolve(dir.path(), &HashMap::new()).await.unwrap();
        assert_eq!(result.get(&url).unwrap(), "nested_pass");
    }
}
