use anyhow::{Ok, Result};
use log::debug;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use tokio::fs;

#[derive(Deserialize, Serialize)]
pub struct GlobalConfig {
    pub user: Option<String>,
    /// Keys this binary does not read, written back untouched. Lade 0.15
    /// fails to load the file without its `update_check`, and older
    /// binaries still share it.
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

impl GlobalConfig {
    pub fn path() -> PathBuf {
        // Allow tests to override the config path without relying on OS-specific
        // path resolution (e.g. SHGetKnownFolderPath on Windows ignores env vars).
        if let std::result::Result::Ok(p) = std::env::var("LADE_CONFIG_PATH") {
            return PathBuf::from(p);
        }
        let project = directories::ProjectDirs::from("com", "zifeo", "lade")
            .expect("cannot get directory for project");
        let config_path = project.config_local_dir().join("config.json");
        debug!("config_path: {:?}", config_path);
        config_path
    }
    pub fn user_from_disk() -> Option<String> {
        let text = std::fs::read_to_string(Self::path()).ok()?;
        serde_json::from_str::<GlobalConfig>(&text)
            .ok()
            .and_then(|c| c.user)
    }

    pub fn os_user() -> Option<String> {
        std::env::var("USER")
            .ok()
            .or_else(|| std::env::var("USERNAME").ok())
    }

    pub fn resolved_user() -> Option<String> {
        Self::user_from_disk().or_else(Self::os_user)
    }

    pub async fn load() -> Result<Self> {
        let path = Self::path();
        if path.exists() {
            let config_str = fs::read_to_string(&path).await?;
            let config: GlobalConfig = serde_json::from_str(&config_str)?;
            Ok(config)
        } else {
            Ok(GlobalConfig {
                user: None,
                rest: serde_json::Map::new(),
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
    fn load_missing_file_has_no_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        temp_env::with_vars([("LADE_CONFIG_PATH", Some(path.to_str().unwrap()))], || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let config = GlobalConfig::load().await.unwrap();
                    assert_eq!(config.user, None);
                    assert!(!path.exists());
                });
        });
    }

    #[test]
    fn resolved_user_falls_back_to_os_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"user":null}"#).unwrap();
        temp_env::with_vars(
            [
                ("LADE_CONFIG_PATH", Some(path.to_str().unwrap())),
                ("USER", Some("alice")),
                ("USERNAME", None),
            ],
            || {
                assert_eq!(GlobalConfig::user_from_disk(), None);
                assert_eq!(GlobalConfig::resolved_user().as_deref(), Some("alice"));
            },
        );
    }

    #[test]
    fn resolved_user_prefers_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"user":"carol"}"#).unwrap();
        temp_env::with_vars(
            [
                ("LADE_CONFIG_PATH", Some(path.to_str().unwrap())),
                ("USER", Some("alice")),
                ("USERNAME", None),
            ],
            || {
                assert_eq!(GlobalConfig::resolved_user().as_deref(), Some("carol"));
            },
        );
    }

    #[test]
    fn save_keeps_keys_older_binaries_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"update_check":"2099-01-01T00:00:00Z","self_version":"0.19.2","user":null,"cli_check":{}}"#,
        )
        .unwrap();
        temp_env::with_vars([("LADE_CONFIG_PATH", Some(path.to_str().unwrap()))], || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    GlobalConfig::update(|c| c.user = Some("carol".into()))
                        .await
                        .unwrap();
                });
        });
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["user"], "carol");
        assert_eq!(saved["update_check"], "2099-01-01T00:00:00Z");
        assert_eq!(saved["self_version"], "0.19.2");
        assert_eq!(saved["cli_check"], serde_json::json!({}));
    }

    #[test]
    fn a_fresh_file_holds_only_the_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        temp_env::with_vars([("LADE_CONFIG_PATH", Some(path.to_str().unwrap()))], || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    GlobalConfig::update(|c| c.user = Some("carol".into()))
                        .await
                        .unwrap();
                });
        });
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved, serde_json::json!({"user": "carol"}));
    }
}
