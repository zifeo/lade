use std::{collections::HashMap, path::Path};

use anyhow::Result;
use async_trait::async_trait;

use crate::Hydration;

mod doppler;
mod file;
mod infisical;
mod onepassword;
mod passbolt;
mod raw;
mod vault;

#[async_trait]
pub trait Provider: Sync {
    fn add(&mut self, value: String) -> Result<()>;
    async fn resolve(&self, cwd: &Path, extra_env: &HashMap<String, String>) -> Result<Hydration>;
}

pub fn providers() -> Vec<Box<dyn Provider + Send>> {
    vec![
        Box::new(doppler::Doppler::new()),
        Box::new(infisical::Infisical::new()),
        Box::new(onepassword::OnePassword::new()),
        Box::new(vault::Vault::new()),
        Box::new(passbolt::Passbolt::new()),
        Box::new(file::File::new()),
        Box::new(raw::Raw::new()),
    ]
}

pub fn envs(extra: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    env.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    env
}
