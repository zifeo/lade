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
    async fn resolve(&self) -> Result<Hydration> {
        let ret = self.values.iter().map(|v| (v.clone(), v.clone())).collect();
        Ok(ret)
    }
}
