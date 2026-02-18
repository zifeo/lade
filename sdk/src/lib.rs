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
    async fn resolve(&self, cwd: &Path, extra_env: &HashMap<String, String>) -> Result<Hydration> {
        Ok(
            try_join_all(self.providers.iter().map(|p| p.resolve(cwd, extra_env)))
                .await?
                .into_iter()
                .flatten()
                .collect(),
        )
    }
}

pub async fn hydrate(
    env: HashMap<String, String>,
    cwd: PathBuf,
    extra_env: HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let mut hydrater = Hydrater::new();
    for value_or_uri in env.values() {
        hydrater.add(value_or_uri.clone())?
    }

    let hydration = hydrater.resolve(&cwd, &extra_env).await?;

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

pub async fn hydrate_one(
    value: String,
    cwd: &Path,
    extra_env: &HashMap<String, String>,
) -> Result<String> {
    let mut hydrater = Hydrater::new();
    hydrater.add(value.clone())?;
    let hydration = hydrater.resolve(cwd, extra_env).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_hydrate_raw_values_with_empty_extra_env() {
        let env = HashMap::from([
            ("KEY1".to_string(), "value1".to_string()),
            ("KEY2".to_string(), "!value2".to_string()),
        ]);
        let result = hydrate(env, PathBuf::from("."), HashMap::new())
            .await
            .unwrap();
        assert_eq!(result.get("KEY1").unwrap(), "value1");
        assert_eq!(result.get("KEY2").unwrap(), "value2");
    }

    #[tokio::test]
    async fn test_hydrate_raw_values_with_extra_env_ignored_by_raw_provider() {
        let env = HashMap::from([("KEY".to_string(), "rawval".to_string())]);
        let extra = HashMap::from([("INJECTED".to_string(), "token123".to_string())]);
        let result = hydrate(env, PathBuf::from("."), extra).await.unwrap();
        assert_eq!(result.get("KEY").unwrap(), "rawval");
    }

    #[tokio::test]
    async fn test_hydrate_one_raw_with_empty_extra_env() {
        let result = hydrate_one("mytoken".to_string(), &PathBuf::from("."), &HashMap::new())
            .await
            .unwrap();
        assert_eq!(result, "mytoken");
    }

    #[tokio::test]
    async fn test_hydrate_one_raw_bang_escape() {
        let result = hydrate_one("!escaped".to_string(), &PathBuf::from("."), &HashMap::new())
            .await
            .unwrap();
        assert_eq!(result, "escaped");
    }
}
