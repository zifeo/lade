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
    pub log: Option<bool>,
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
#[path = "secret_tests.rs"]
mod tests;
