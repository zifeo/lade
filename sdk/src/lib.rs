use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Ok, Result, bail};
use futures::future::try_join_all;
use once_cell::sync::Lazy;
use providers::Provider;
use regex::Regex;

type Hydration = HashMap<String, String>;

mod providers;

pub struct Hydrater {
    providers: Vec<Box<dyn Provider + Send>>,
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
    async fn resolve(&self, cwd: &Path) -> Result<Hydration> {
        Ok(try_join_all(self.providers.iter().map(|p| p.resolve(cwd)))
            .await?
            .into_iter()
            .flatten()
            .collect())
    }
}

pub async fn hydrate(
    env: HashMap<String, String>,
    cwd: PathBuf,
) -> Result<HashMap<String, String>> {
    let mut hydrater = Hydrater::new();
    for value_or_uri in env.values() {
        hydrater.add(value_or_uri.clone())?
    }

    let hydration = hydrater.resolve(&cwd).await?;

    let mut ret: HashMap<String, String> = HashMap::default();
    for (key, value_or_uri) in env.iter() {
        ret.insert(
            key.clone(),
            hydration
                .get(value_or_uri)
                .unwrap_or_else(|| {
                    panic!(
                        "Cannot find {} in {}",
                        value_or_uri,
                        hydration
                            .keys()
                            .cloned()
                            .collect::<Vec<String>>()
                            .join(", ")
                    )
                })
                .clone(),
        );
    }

    Ok(ret)
}

pub async fn hydrate_one(value: String, cwd: &Path) -> Result<String> {
    let mut hydrater = Hydrater::new();
    hydrater.add(value.clone())?;
    let hydration = hydrater.resolve(cwd).await?;
    let hydrated = hydration.get(&value).unwrap().to_owned();
    Ok(hydrated)
}

static VAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\$\{?(\w+)\}?)").unwrap());

pub fn resolve(
    kvs: &HashMap<String, String>,
    existing_vars: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    kvs.iter()
        .map(|(key, value)| resolve_one(value, existing_vars).map(|v| (key.clone(), v)))
        .collect()
}

pub fn resolve_one(value: &str, existing_vars: &HashMap<String, String>) -> Result<String> {
    Ok(VAR.captures_iter(value).fold(value.to_string(), |agg, c| {
        agg.replace(&c[1], existing_vars.get(&c[2]).unwrap_or(&"".to_string()))
    }))
}
