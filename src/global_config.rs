use anyhow::{Ok, Result};
use chrono::{DateTime, Utc};
use log::debug;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use tokio::fs;

#[derive(Deserialize, Serialize)]
pub struct GlobalConfig {
    /// When we last *attempted* a GitHub version check. `None` means never.
    /// Never invent `Utc::now()` here: that skipped the real lookup for 24h.
    #[serde(default)]
    pub update_check: Option<DateTime<Utc>>,
    /// Last GitHub latest tag we successfully fetched. Separate from
    /// `update_check`, which is stamped even when the request fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    pub user: Option<String>,
    #[serde(default)]
    pub cli_check: BTreeMap<String, DateTime<Utc>>,
}

impl GlobalConfig {
    pub fn path() -> PathBuf {
        // Allow tests to override the config path without relying on OS-specific
        // path resolution (e.g. SHGetKnownFolderPath on Windows ignores env vars).
        if let std::result::Result::Ok(p) = std::env::var("LADE_CONFIG_PATH") {
            return PathBuf::from(p);
        }
        let project = directories::ProjectDirs::from("com", "zifeo", "lade")
            .expect("cannot get directory for projet");
        let config_path = project.config_local_dir().join("config.json");
        debug!("config_path: {:?}", config_path);
        config_path
    }
    pub async fn load() -> Result<Self> {
        let path = Self::path();
        if path.exists() {
            let config_str = fs::read_to_string(&path).await?;
            let config: GlobalConfig = serde_json::from_str(&config_str)?;
            Ok(config)
        } else {
            Ok(GlobalConfig {
                update_check: None,
                latest_version: None,
                user: None,
                cli_check: BTreeMap::new(),
            })
        }
    }

    pub async fn update<F: FnOnce(&mut GlobalConfig)>(f: F) -> Result<()> {
        let mut config = Self::load().await?;
        f(&mut config);
        config.save().await?;
        Ok(())
    }

    async fn save(&self) -> Result<()> {
        let config_str = serde_json::to_string_pretty(&self)?;
        let path = Self::path();
        let tmp = path.with_file_name(format!(
            "{}.{}.tmp",
            path.file_name().unwrap().to_string_lossy(),
            std::process::id(),
        ));
        fs::create_dir_all(path.parent().unwrap()).await?;
        fs::write(&tmp, &config_str).await?;
        fs::rename(&tmp, &path).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_is_never_checked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        temp_env::with_vars([("LADE_CONFIG_PATH", Some(path.to_str().unwrap()))], || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let config = GlobalConfig::load().await.unwrap();
                    assert_eq!(config.update_check, None);
                    assert!(!path.exists());
                });
        });
    }

    #[test]
    fn load_legacy_datetime_update_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"update_check":"2099-01-01T00:00:00Z","user":null,"cli_check":{}}"#,
        )
        .unwrap();
        temp_env::with_vars([("LADE_CONFIG_PATH", Some(path.to_str().unwrap()))], || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let config = GlobalConfig::load().await.unwrap();
                    assert!(config.update_check.is_some());
                    assert_eq!(config.latest_version, None);
                });
        });
    }
}
