use std::{collections::HashMap, path::Path};

use anyhow::{Ok, Result};
use async_trait::async_trait;

use super::Provider;
use crate::Hydration;

#[derive(Default)]
pub struct Raw {
    values: Vec<String>,
}

impl Raw {
    pub fn new() -> Self {
        Default::default()
    }
}

#[async_trait]
impl Provider for Raw {
    fn add(&mut self, value: String) -> Result<()> {
        self.values.push(value);
        Ok(())
    }
    async fn resolve(&self, _: &Path, _: &HashMap<String, String>) -> Result<Hydration> {
        let ret = self
            .values
            .iter()
            .map(|v| {
                let mut value = v.clone();
                // escape the first ! if it exists
                if value.starts_with('!') {
                    value.remove(0);
                }
                (v.clone(), value)
            })
            .collect();
        Ok(ret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    #[test]
    fn test_add_accepts_any_value() {
        let mut p = Raw::new();
        assert!(p.add("plain_value".to_string()).is_ok());
        assert!(p.add("vault://host/path".to_string()).is_ok());
        assert!(p.add("!escaped".to_string()).is_ok());
        assert!(p.add("".to_string()).is_ok());
    }

    #[tokio::test]
    async fn test_resolve_strips_bang_prefix() {
        let mut p = Raw::new();
        p.add("!escaped_value".to_string()).unwrap();
        let result = p.resolve(Path::new("."), &HashMap::new()).await.unwrap();
        assert_eq!(result.get("!escaped_value").unwrap(), "escaped_value");
    }

    #[tokio::test]
    async fn test_resolve_plain_value_unchanged() {
        let mut p = Raw::new();
        p.add("plain_value".to_string()).unwrap();
        let result = p.resolve(Path::new("."), &HashMap::new()).await.unwrap();
        assert_eq!(result.get("plain_value").unwrap(), "plain_value");
    }
}
