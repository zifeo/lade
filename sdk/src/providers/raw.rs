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
