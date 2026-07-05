use anyhow::{Result, bail};
use std::collections::HashMap;

use super::{NetworkProvider, ProviderSpec, reject_unknown_query};

pub struct TshProvider;

impl NetworkProvider for TshProvider {
    fn scheme(&self) -> &'static str {
        "tsh"
    }

    fn parse(
        &self,
        authority: &str,
        segments: &[&str],
        query: &HashMap<String, String>,
    ) -> Result<ProviderSpec> {
        if segments.len() != 5 {
            bail!("tsh URI must be /<kube-cluster>/<namespace>/<kind>/<name>/<remote-port>");
        }
        reject_unknown_query(query, &["local"])?;
        Ok(ProviderSpec::Tsh {
            teleport_proxy: authority.to_string(),
            kube_cluster: segments[0].to_string(),
            namespace: segments[1].to_string(),
            kind: segments[2].to_string(),
            name: segments[3].to_string(),
            remote_port: segments[4].parse::<u16>()?,
        })
    }
}
