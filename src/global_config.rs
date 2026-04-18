use anyhow::{Ok, Result};
use chrono::{DateTime, Utc};
use log::debug;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use tokio::fs;

#[derive(Deserialize, Serialize)]
pub struct GlobalConfig {
    pub update_check: DateTime<Utc>,
    pub user: Option<String>,
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
                update_check: Utc::now(),
                user: None,
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
