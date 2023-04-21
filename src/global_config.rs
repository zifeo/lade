use anyhow::{Ok, Result};
use chrono::{DateTime, Utc};

use serde::{Deserialize, Serialize};
use std::path::Path;

use tokio::fs;

#[derive(Deserialize, Serialize)]
pub struct GlobalConfig {
    pub update_check: DateTime<Utc>,
}

impl GlobalConfig {
    pub async fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        if path.as_ref().exists() {
            let config_str = fs::read_to_string(path).await?;
            let config: GlobalConfig = serde_json::from_str(&config_str)?;
            Ok(config)
        } else {
            let config = GlobalConfig {
                update_check: Utc::now(),
            };
            config.save(path).await?;
            Ok(config)
        }
    }

    pub async fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let config_str = serde_json::to_string_pretty(&self)?;
        fs::create_dir_all(path.as_ref().parent().unwrap()).await?;
        fs::write(path, config_str).await?;
        Ok(())
    }
}
