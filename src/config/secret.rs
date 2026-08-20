use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::de;

#[derive(Debug, Clone)]
pub enum LadeSecret {
    Secret(String),
    User(HashMap<String, Option<String>>),
    Unset,
}

impl<'de> Deserialize<'de> for LadeSecret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        if value.is_null() {
            return Ok(LadeSecret::Unset);
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Secret(String),
            User(HashMap<String, Option<String>>),
        }
        match Repr::deserialize(value) {
            Ok(Repr::Secret(value)) => Ok(LadeSecret::Secret(value)),
            Ok(Repr::User(map)) => Ok(LadeSecret::User(map)),
            Err(error) => Err(de::Error::custom(error)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleWhen {
    #[default]
    Always,
    Human,
    Agent,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct RuleConfig {
    pub file: Option<PathBuf>,
    #[serde(rename = "1password_service_account")]
    pub onepassword_service_account: Option<LadeSecret>,
    pub disclaimer: Option<String>,
    #[serde(default)]
    pub when: RuleWhen,
    #[serde(default)]
    pub silence: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LadeRule {
    #[serde(rename = ".")]
    pub config: Option<RuleConfig>,
    #[serde(flatten, deserialize_with = "deserialize_rule_entries")]
    pub secrets: HashMap<String, LadeSecret>,
}

fn deserialize_rule_entries<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, LadeSecret>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: HashMap<serde_yaml::Value, LadeSecret> = HashMap::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(key, value)| {
            let key = match key {
                serde_yaml::Value::String(s) => s,
                serde_yaml::Value::Number(n) => n.to_string(),
                other => {
                    return Err(de::Error::custom(format!(
                        "invalid key type in rule entries: {other:?}"
                    )));
                }
            };
            Ok((key, value))
        })
        .collect()
}

pub(super) fn resolve_lade_secret(secret: &LadeSecret, user: &Option<String>) -> Option<String> {
    match secret {
        LadeSecret::Secret(value) => Some(value.clone()),
        LadeSecret::User(map) => user
            .as_ref()
            .and_then(|u| map.get(u))
            .or_else(|| map.get("."))
            .and_then(|v| v.clone()),
        LadeSecret::Unset => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LadeFile;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_lade_secret_string() {
        let secret = LadeSecret::Secret("value".to_string());
        assert_eq!(
            resolve_lade_secret(&secret, &Some("any".to_string())),
            Some("value".to_string())
        );
        assert_eq!(
            resolve_lade_secret(&secret, &None),
            Some("value".to_string())
        );
    }

    #[test]
    fn test_resolve_lade_secret_user_match() {
        let mut map = HashMap::new();
        map.insert("zifeo".to_string(), Some("secret_for_zifeo".to_string()));
        map.insert(".".to_string(), Some("default_secret".to_string()));
        let secret = LadeSecret::User(map);
        assert_eq!(
            resolve_lade_secret(&secret, &Some("zifeo".to_string())),
            Some("secret_for_zifeo".to_string())
        );
    }

    #[test]
    fn test_resolve_lade_secret_user_default_fallback() {
        let mut map = HashMap::new();
        map.insert("zifeo".to_string(), Some("secret_for_zifeo".to_string()));
        map.insert(".".to_string(), Some("default_secret".to_string()));
        let secret = LadeSecret::User(map);
        assert_eq!(
            resolve_lade_secret(&secret, &Some("other_user".to_string())),
            Some("default_secret".to_string())
        );
        assert_eq!(
            resolve_lade_secret(&secret, &None),
            Some("default_secret".to_string())
        );
    }

    #[test]
    fn test_resolve_lade_secret_user_no_match_no_default() {
        let mut map = HashMap::new();
        map.insert("zifeo".to_string(), Some("secret_for_zifeo".to_string()));
        let secret = LadeSecret::User(map);
        assert_eq!(
            resolve_lade_secret(&secret, &Some("other".to_string())),
            None
        );
        assert_eq!(resolve_lade_secret(&secret, &None), None);
    }

    #[test]
    fn test_resolve_lade_secret_user_null_default() {
        let mut map = HashMap::new();
        map.insert("zifeo".to_string(), Some("secret_for_zifeo".to_string()));
        map.insert(".".to_string(), None);
        let secret = LadeSecret::User(map);
        assert_eq!(
            resolve_lade_secret(&secret, &Some("other".to_string())),
            None
        );
        assert_eq!(resolve_lade_secret(&secret, &None), None);
    }

    #[test]
    fn test_yaml_null_and_tilde_are_unset() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            "\"cmd\":\n  VIA_TILDE: ~\n  VIA_NULL: null\n  VIA_EMPTY:\n  VIA_STRING: \"\"\n  KEEP: val\n",
        )
        .unwrap();
        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let secrets = &lade_file.commands.get("cmd").unwrap()[0].secrets;
        assert!(matches!(secrets.get("VIA_TILDE"), Some(LadeSecret::Unset)));
        assert!(matches!(secrets.get("VIA_NULL"), Some(LadeSecret::Unset)));
        assert!(matches!(secrets.get("VIA_EMPTY"), Some(LadeSecret::Unset)));
        assert!(matches!(
            secrets.get("VIA_STRING"),
            Some(LadeSecret::Secret(value)) if value.is_empty()
        ));
        assert!(matches!(
            secrets.get("KEEP"),
            Some(LadeSecret::Secret(value)) if value == "val"
        ));
    }

    #[test]
    fn test_rule_config_silence() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            "\"cmd\":\n  \".\":\n    silence: true\n  KEY: val\n",
        )
        .unwrap();
        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let config = lade_file.commands.get("cmd").unwrap()[0]
            .config
            .as_ref()
            .unwrap();
        assert!(config.silence);
    }

    #[test]
    fn test_rule_config_when() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            "\"cmd\":\n  \".\":\n    when: agent\n  KEY: val\n",
        )
        .unwrap();
        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let config = lade_file.commands.get("cmd").unwrap()[0]
            .config
            .as_ref()
            .unwrap();
        assert_eq!(config.when, RuleWhen::Agent);
    }

    #[test]
    fn test_rule_config_when_invalid_fails() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            "\"cmd\":\n  \".\":\n    when: robot\n  KEY: val\n",
        )
        .unwrap();
        assert!(LadeFile::from_path(&file_path).is_err());
    }

    #[test]
    fn test_lade_secrets_on_yaml() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            b"\"test command\":\n  \".\": { file: \"output/path\" }\n  secret1: \"secret_value\"\n  secret2:\n    user: \"user_name\"\n    password: \"password_value\"\n",
        ).unwrap();

        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let command = &lade_file.commands.get("test command").unwrap()[0];
        assert_eq!(
            command.config.as_ref().unwrap().file,
            Some(PathBuf::from("output/path"))
        );
        assert!(
            command
                .config
                .as_ref()
                .unwrap()
                .onepassword_service_account
                .is_none()
        );

        let secrets = &command.secrets;
        assert_eq!(secrets.len(), 2);

        if let LadeSecret::Secret(value) = secrets.get("secret1").unwrap() {
            assert_eq!(value, "secret_value");
        } else {
            panic!("secret1 should be a LadeSecret::Secret");
        }

        if let LadeSecret::User(map) = secrets.get("secret2").unwrap() {
            let mut expected = HashMap::new();
            expected.insert("user".to_string(), Some("user_name".to_string()));
            expected.insert("password".to_string(), Some("password_value".to_string()));
            assert_eq!(*map, expected);
        } else {
            panic!("secret2 should be a LadeSecret::User");
        }
    }

    #[test]
    fn test_rule_config_sa_string() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            b"\"cmd\":\n  \".\":\n    1password_service_account: \"op://host/vault/item\"\n  KEY: val\n",
        ).unwrap();
        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let rule = &lade_file.commands.get("cmd").unwrap()[0];
        let config = rule.config.as_ref().unwrap();
        assert!(config.file.is_none());
        assert!(matches!(
            config.onepassword_service_account.as_ref().unwrap(),
            LadeSecret::Secret(s) if s == "op://host/vault/item"
        ));
    }

    #[test]
    fn test_rule_config_sa_user_map_with_default() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("lade.yml");
        std::fs::write(
            &file_path,
            b"\"cmd\":\n  \".\":\n    1password_service_account:\n      zifeo: \"op://host/vault/item\"\n      \".\": null\n  KEY: val\n",
        ).unwrap();
        let lade_file = LadeFile::from_path(&file_path).unwrap();
        let rule = &lade_file.commands.get("cmd").unwrap()[0];
        let config = rule.config.as_ref().unwrap();
        if let LadeSecret::User(map) = config.onepassword_service_account.as_ref().unwrap() {
            assert_eq!(
                map.get("zifeo"),
                Some(&Some("op://host/vault/item".to_string()))
            );
            assert_eq!(map.get("."), Some(&None));
        } else {
            panic!("expected LadeSecret::User");
        }
    }
}
