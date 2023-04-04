use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    str::FromStr,
};

use access_json::JSONQuery;
use anyhow::{bail, Context, Ok, Result};
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
    urls: Vec<String>,
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
                self.urls.push(value);
                Ok(())
            }
            _ => bail!("Not an file scheme or missing ?query=.field part"),
        }
    }
    async fn resolve(&self, cwd: &Path) -> Result<Hydration> {
        let home = env::var("HOME").context("getting $HOME")?;
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|u| {
                let u = Url::parse(u).unwrap();
                let port = match u.port() {
                    Some(port) => format!(":{}", port),
                    None => "".to_string(),
                };
                format!(
                    "{}{}{}",
                    u.host()
                        .map(|h| h.to_string().replace("$home", &home).replace('~', &home))
                        .unwrap_or("".to_string()),
                    port,
                    u.path()
                )
            })
            .into_iter()
            .map(|(file, group)| {
                let mut path = PathBuf::from_str(&file).unwrap();
                if !file.starts_with('/') {
                    path = cwd.join(path);
                }
                let format = file
                    .split('.')
                    .last()
                    .expect("no file format found")
                    .to_string();

                async move {
                    let str = fs::read_to_string(&path)
                        .await
                        .unwrap_or_else(|_| panic!("cannot read file {}", path.display()));
                    let json = match format.as_str() {
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
                        .map(|u| {
                            let url = Url::parse(u).unwrap();
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

                            (u.to_string(), output)
                        })
                        .collect::<Hydration>();

                    Ok(hydration)
                }
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
