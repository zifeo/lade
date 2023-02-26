use std::collections::HashMap;

use anyhow::{bail, Ok, Result};
use futures::future::try_join_all;
use once_cell::sync::Lazy;
use providers::Provider;
use regex::Regex;

type Hydration = HashMap<String, String>;

mod providers;

pub struct Hydrater {
    providers: Vec<Box<dyn Provider>>,
}

impl Hydrater {
    fn new() -> Self {
        Self {
            providers: providers::providers(),
        }
    }
    fn add(&mut self, value: String) -> Result<()> {
        for provider in self.providers.iter_mut() {
            if provider.add(value.clone()).is_ok() {
                return Ok(());
            }
        }
        bail!("No provider found")
    }
    async fn resolve(&self) -> Result<Hydration> {
        Ok(try_join_all(self.providers.iter().map(|p| p.resolve()))
            .await?
            .into_iter()
            .flatten()
            .collect())
    }
}

pub async fn hydrate_one(value: String) -> Result<String> {
    let mut hydrater = Hydrater::new();
    hydrater.add(value.clone())?;
    let hydration = hydrater.resolve().await?;
    let hydrated = hydration.get(&value).unwrap().to_owned();
    Ok(hydrated)
}

pub async fn hydrate(env: HashMap<String, String>) -> Result<HashMap<String, String>> {
    let mut hydrater = Hydrater::new();
    for value_or_uri in env.values() {
        hydrater.add(value_or_uri.clone())?
    }

    let hydration = hydrater.resolve().await?;

    let mut ret: HashMap<String, String> = HashMap::default();
    for (key, value_or_uri) in env.iter() {
        ret.insert(key.clone(), hydration.get(value_or_uri).unwrap().clone());
    }

    Ok(ret)
}

static VAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\$\{?(\w+)\}?)").unwrap());

pub fn resolve_env(
    kvs: &HashMap<String, String>,
    existing_vars: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let ret = kvs
        .iter()
        .map(|(key, value)| {
            let hydration = VAR.captures_iter(value).fold(value.clone(), |agg, c| {
                agg.replace(&c[1], existing_vars.get(&c[2]).unwrap_or(&"".to_string()))
            });
            (key.clone(), hydration)
        })
        .collect();
    Ok(ret)
}
