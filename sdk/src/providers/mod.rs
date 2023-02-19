use anyhow::Result;
use async_trait::async_trait;

use crate::Hydration;

mod doppler;
mod infisical;
mod onepassword;
mod raw;

#[async_trait]
pub trait Provider: Sync {
    fn add(&mut self, value: String) -> Result<()>;
    async fn resolve(&self) -> Result<Hydration>;
}

pub fn providers() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(doppler::Doppler::new()),
        Box::new(infisical::Infisical::new()),
        Box::new(onepassword::OnePassword::new()),
        Box::new(raw::Raw::new()),
    ]
}
