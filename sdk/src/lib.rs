use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Ok, Result};
use providers::Providers;
use rustc_hash::FxHashMap;

mod providers;
mod resolve;

pub use resolve::{resolve, resolve_one};

type Hydration = FxHashMap<String, String>;

pub async fn hydrate(
    env: HashMap<String, String>,
    cwd: PathBuf,
    extra_env: HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let mut providers = Providers::new();
    for value_or_uri in env.values() {
        providers.add(value_or_uri.clone())?;
    }

    let mut hydration = providers.resolve(&cwd, &extra_env).await?;

    Ok(env
        .into_iter()
        .map(|(key, value_or_uri)| {
            let value = hydration.remove(&value_or_uri).unwrap_or_else(|| {
                panic!(
                    "Cannot find {} in {}",
                    value_or_uri,
                    hydration
                        .keys()
                        .cloned()
                        .collect::<Vec<String>>()
                        .join(", ")
                )
            });
            (key, value)
        })
        .collect())
}

pub async fn hydrate_one(
    value: String,
    cwd: &Path,
    extra_env: &HashMap<String, String>,
) -> Result<String> {
    let mut providers = Providers::new();
    providers.add(value.clone())?;
    let hydration = providers.resolve(cwd, extra_env).await?;
    Ok(hydration.get(&value).unwrap().to_owned())
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
