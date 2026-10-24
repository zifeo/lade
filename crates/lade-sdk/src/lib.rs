use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Ok, Result, bail};
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(feature = "mcp")]
pub mod mcp;
mod providers;
mod resolve;

pub use providers::Provider;
pub use providers::Providers;
pub use providers::Transport;
pub use providers::Warnings;
pub use providers::add::{AddField, secret_add_fields, secret_search_scope};
pub use providers::compat;
pub use providers::compose_secret_add_uri;
#[cfg(feature = "network")]
pub use providers::network;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub use resolve::{Dag, Template, resolve, resolve_one};

type Hydration = FxHashMap<String, String>;

/// Warnings are returned, not printed: a caller decides how to surface them.
#[derive(Debug, PartialEq, Eq)]
pub struct Hydrated {
    pub values: HashMap<String, String>,
    pub warnings: Vec<String>,
}

/// Duplicate sources are resolved once, distinct ones concurrently.
pub async fn hydrate_many(
    env: HashMap<String, String>,
    cwd: PathBuf,
    extra_env: HashMap<String, String>,
) -> Result<Hydrated> {
    let (values, _maskable, warnings) = hydrate_with_maskable(env, cwd, extra_env).await?;
    Ok(Hydrated { values, warnings })
}

pub async fn hydrate_with_maskable(
    env: HashMap<String, String>,
    cwd: PathBuf,
    extra_env: HashMap<String, String>,
) -> Result<(HashMap<String, String>, FxHashSet<String>, Vec<String>)> {
    let mut providers = Providers::new();
    for value_or_uri in env.values() {
        providers.add(value_or_uri.clone())?;
    }
    let warnings = Warnings::default();
    let (hydration, maskable) = providers.resolve(&cwd, &extra_env, &warnings).await?;

    let mut values = HashMap::with_capacity(env.len());
    for (key, value_or_uri) in env {
        let Some(value) = hydration.get(&value_or_uri) else {
            bail!(
                "Cannot find {} in {}",
                value_or_uri,
                hydration
                    .keys()
                    .cloned()
                    .collect::<Vec<String>>()
                    .join(", ")
            );
        };
        values.insert(key, value.clone());
    }

    Ok((values, maskable, warnings.take()))
}

pub async fn hydrate_one(
    value: String,
    cwd: &Path,
    extra_env: &HashMap<String, String>,
) -> Result<String> {
    let mut providers = Providers::new();
    providers.add(value.clone())?;
    let (hydration, _) = providers
        .resolve(cwd, extra_env, &Warnings::default())
        .await?;
    let Some(value) = hydration.get(&value) else {
        bail!("Cannot find {value} in the resolved providers");
    };
    Ok(value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_hydrate_many_returns_values_and_warnings() {
        let env = HashMap::from([
            ("KEY1".to_string(), "value1".to_string()),
            ("KEY2".to_string(), "value2".to_string()),
        ]);
        let hydrated = hydrate_many(env, PathBuf::from("."), HashMap::new())
            .await
            .unwrap();
        assert_eq!(hydrated.values.get("KEY1").unwrap(), "value1");
        assert_eq!(hydrated.values.get("KEY2").unwrap(), "value2");
        assert!(hydrated.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_hydrate_raw_values_with_empty_extra_env() {
        let env = HashMap::from([
            ("KEY1".to_string(), "value1".to_string()),
            ("KEY2".to_string(), "!value2".to_string()),
        ]);
        let (values, maskable, _warnings) =
            hydrate_with_maskable(env, PathBuf::from("."), HashMap::new())
                .await
                .unwrap();
        assert_eq!(values.get("KEY1").unwrap(), "value1");
        assert_eq!(values.get("KEY2").unwrap(), "value2");
        assert!(maskable.is_empty());
    }

    #[tokio::test]
    async fn test_hydrate_raw_values_with_extra_env_ignored_by_raw_provider() {
        let env = HashMap::from([("KEY".to_string(), "rawval".to_string())]);
        let extra = HashMap::from([("INJECTED".to_string(), "token123".to_string())]);
        let (values, maskable, _warnings) = hydrate_with_maskable(env, PathBuf::from("."), extra)
            .await
            .unwrap();
        assert_eq!(values.get("KEY").unwrap(), "rawval");
        assert!(maskable.is_empty());
    }

    #[tokio::test]
    async fn test_hydrate_one_raw_with_empty_extra_env() {
        let result = hydrate_one("mytoken".to_string(), &PathBuf::from("."), &HashMap::new())
            .await
            .unwrap();
        assert_eq!(result, "mytoken");
    }

    #[tokio::test]
    async fn test_hydrate_duplicate_raw_values() {
        let env = HashMap::from([
            ("KEY1".to_string(), "a".to_string()),
            ("KEY2".to_string(), "a".to_string()),
        ]);
        let (values, _, _warnings) = hydrate_with_maskable(env, PathBuf::from("."), HashMap::new())
            .await
            .unwrap();
        assert_eq!(values.get("KEY1").unwrap(), "a");
        assert_eq!(values.get("KEY2").unwrap(), "a");
    }

    #[tokio::test]
    async fn test_hydrate_one_raw_bang_escape() {
        let result = hydrate_one("!escaped".to_string(), &PathBuf::from("."), &HashMap::new())
            .await
            .unwrap();
        assert_eq!(result, "escaped");
    }

    #[tokio::test]
    async fn test_hydrate_file_and_shell_in_one_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg.json");
        std::fs::write(&path, r#"{"k":"from-file"}"#).unwrap();
        let file = format!("file://{}?query=.k", path.display());
        let env = HashMap::from([
            ("FROM_FILE".to_string(), file),
            (
                "FROM_SH".to_string(),
                "sh://printf '%s' from-shell".to_string(),
            ),
        ]);
        let (values, maskable, _) =
            hydrate_with_maskable(env, dir.path().to_path_buf(), HashMap::new())
                .await
                .unwrap();
        assert_eq!(values.get("FROM_FILE").unwrap(), "from-file");
        assert_eq!(values.get("FROM_SH").unwrap(), "from-shell");
        assert_eq!(maskable.len(), 2);
    }

    #[tokio::test]
    async fn test_hydrate_fail_closed_across_schemes() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.json");
        let env = HashMap::from([
            (
                "BAD".to_string(),
                format!("file://{}?query=.k", missing.display()),
            ),
            ("OK".to_string(), "sh://printf '%s' unused".to_string()),
        ]);
        let err = hydrate_with_maskable(env, dir.path().to_path_buf(), HashMap::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cannot read file"), "{err}");
    }
}
