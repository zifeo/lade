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
