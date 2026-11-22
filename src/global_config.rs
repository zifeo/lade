use anyhow::{Context, Result};
use log::debug;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use tokio::fs;

#[derive(Deserialize, Serialize)]
pub struct GlobalConfig {
    pub user: Option<String>,
    // § one pq5ebbcs0.a@jfge2
    /// Cap on what the hub keeps after a command. Kept raw: a bad value must
    /// not stop `user` from loading. See [`max_cache_ms`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cache: Option<serde_json::Value>,
    // § one pq5ebbcs0.b@wm64r
    /// Keys this binary does not read, written back untouched. Lade 0.15
    /// fails to load the file without its `update_check`, and older
    /// binaries still share it.
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

impl GlobalConfig {
    pub fn path() -> PathBuf {
        // § pq5ebbcs0.c@fdq7n
        // Allow tests to override the config path without relying on OS-specific
        // path resolution (e.g. SHGetKnownFolderPath on Windows ignores env vars).
        if let Ok(p) = std::env::var("LADE_CONFIG_PATH") {
            return PathBuf::from(p);
        }
        #[cfg(test)]
        if crate::test_state::uses_real_home() {
            return crate::test_state::dir().join("config.json");
        }
        let project = directories::ProjectDirs::from("com", "zifeo", "lade")
            .expect("cannot get directory for project");
        let config_path = project.config_local_dir().join("config.json");
        debug!("config_path: {:?}", config_path);
        config_path
    }
    pub fn os_user() -> Option<String> {
        std::env::var("USER")
            .ok()
            .filter(|user| !user.is_empty())
            .or_else(|| std::env::var("USERNAME").ok())
            .filter(|user| !user.is_empty())
    }

    /// The one resolution of the Lade user, on every path.
    pub fn resolved_user() -> Result<Option<String>> {
        // § dcrxp986s.b@9aqw0
        let path = Self::path();
        let saved = match std::fs::read_to_string(&path) {
            Ok(text) => {
                serde_json::from_str::<GlobalConfig>(&text)
                    .with_context(|| format!("could not parse {}", path.display()))?
                    .user
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(e).with_context(|| format!("could not read {}", path.display()));
            }
        };
        Ok(saved.or_else(Self::os_user))
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
                max_cache: None,
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

/// `max_cache` in ms. `None`: no field, nothing capped. `Some(0)`: the cache
/// is off. The hub reads an `Err` as off too.
pub fn max_cache_ms(value: Option<&serde_json::Value>) -> Result<Option<u32>, String> {
    const HINT: &str = "max_cache is a duration like \"1h\", or \"0\" to turn the cache off";
    // § cfan04ksd.c@91e64
    let raw = match value {
        None | Some(serde_json::Value::Null) => return Ok(None),
        Some(serde_json::Value::String(raw)) => raw.trim(),
        Some(serde_json::Value::Number(n)) if n.as_u64() == Some(0) => return Ok(Some(0)),
        Some(other) => return Err(format!("{HINT}, not {other}")),
    };
    if raw == "0" {
        return Ok(Some(0));
    }
    match crate::config::RuleTtl::parse(raw) {
        Ok(crate::config::RuleTtl::Off) => Ok(Some(0)),
        Ok(crate::config::RuleTtl::Window(duration)) => {
            Ok(Some(crate::config::duration_to_ttl_ms(duration)))
        }
        Err(e) => Err(format!("{HINT}, not {raw:?}: {e}")),
    }
}

/// [`max_cache_ms`] from the raw `config.json` text.
pub fn max_cache_from_text(text: &str) -> Result<Option<u32>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("config.json: {e}"))?;
    let serde_json::Value::Object(map) = value else {
        return Err("config.json is not a JSON object".to_string());
    };
    max_cache_ms(map.get("max_cache"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_cache_parses_off_durations_and_rejects_the_rest() {
        assert_eq!(max_cache_from_text("{}"), Ok(None));
        assert_eq!(max_cache_from_text(r#"{"max_cache":null}"#), Ok(None));
        assert_eq!(max_cache_from_text(r#"{"max_cache":"0"}"#), Ok(Some(0)));
        assert_eq!(max_cache_from_text(r#"{"max_cache":0}"#), Ok(Some(0)));
        assert_eq!(max_cache_from_text(r#"{"max_cache":"off"}"#), Ok(Some(0)));
        assert_eq!(
            max_cache_from_text(r#"{"max_cache":"1h"}"#),
            Ok(Some(3_600_000))
        );
        assert!(max_cache_from_text(r#"{"max_cache":"48h"}"#).is_err());
        assert!(max_cache_from_text(r#"{"max_cache":"soon"}"#).is_err());
        assert!(max_cache_from_text(r#"{"max_cache":3600}"#).is_err());
        assert!(max_cache_from_text("[]").is_err());
        assert!(max_cache_from_text("{").is_err());
    }

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
                assert_eq!(
                    GlobalConfig::resolved_user().unwrap().as_deref(),
                    Some("alice")
                );
            },
        );
    }

    #[test]
    fn resolved_user_skips_an_empty_os_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        temp_env::with_vars(
            [
                ("LADE_CONFIG_PATH", Some(path.to_str().unwrap())),
                ("USER", Some("")),
                ("USERNAME", Some("bob")),
            ],
            || {
                assert_eq!(
                    GlobalConfig::resolved_user().unwrap().as_deref(),
                    Some("bob")
                );
            },
        );
        temp_env::with_vars(
            [
                ("LADE_CONFIG_PATH", Some(path.to_str().unwrap())),
                ("USER", Some("")),
                ("USERNAME", None),
            ],
            || assert_eq!(GlobalConfig::resolved_user().unwrap(), None),
        );
    }

    #[test]
    fn resolved_user_fails_on_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{").unwrap();
        temp_env::with_vars(
            [
                ("LADE_CONFIG_PATH", Some(path.to_str().unwrap())),
                ("USER", Some("alice")),
                ("USERNAME", None),
            ],
            || {
                let err = GlobalConfig::resolved_user().unwrap_err();
                assert!(format!("{err:#}").contains("could not parse"), "{err:#}");
            },
        );
    }

    #[test]
    fn resolved_user_without_file_is_the_os_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        temp_env::with_vars(
            [
                ("LADE_CONFIG_PATH", Some(path.to_str().unwrap())),
                ("USER", Some("alice")),
                ("USERNAME", None),
            ],
            || {
                assert_eq!(
                    GlobalConfig::resolved_user().unwrap().as_deref(),
                    Some("alice")
                );
            },
        );
    }

    #[test]
    fn bad_max_cache_keeps_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"user":"carol","max_cache":"soon"}"#).unwrap();
        temp_env::with_vars(
            [
                ("LADE_CONFIG_PATH", Some(path.to_str().unwrap())),
                ("USER", Some("alice")),
                ("USERNAME", None),
            ],
            || {
                assert_eq!(
                    GlobalConfig::resolved_user().unwrap().as_deref(),
                    Some("carol")
                );
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
                assert_eq!(
                    GlobalConfig::resolved_user().unwrap().as_deref(),
                    Some("carol")
                );
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
