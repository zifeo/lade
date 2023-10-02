use std::{collections::HashMap, path::Path};

use anyhow::Result;
use async_trait::async_trait;

use crate::Hydration;

mod doppler;
mod file;
mod infisical;
mod onepassword;
mod raw;
mod vault;

#[async_trait]
pub trait Provider: Sync {
    fn add(&mut self, value: String) -> Result<()>;
    async fn resolve(&self, cwd: &Path) -> Result<Hydration>;
}

pub fn providers() -> Vec<Box<dyn Provider + Send>> {
    vec![
        Box::new(doppler::Doppler::new()),
        Box::new(infisical::Infisical::new()),
        Box::new(onepassword::OnePassword::new()),
        Box::new(vault::Vault::new()),
        Box::new(file::File::new()),
        Box::new(raw::Raw::new()),
    ]
}

pub fn envs() -> HashMap<String, String> {
    std::env::vars().collect()
}
