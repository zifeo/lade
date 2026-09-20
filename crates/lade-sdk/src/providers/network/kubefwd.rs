use anyhow::{Result, bail};
use std::collections::HashMap;

use super::{NetworkProvider, ProviderSpec, reject_unknown_query};

pub struct KubefwdProvider;

impl NetworkProvider for KubefwdProvider {
    fn scheme(&self) -> &'static str {
        "kubefwd"
    }

    fn parse(
        &self,
        authority: &str,
        segments: &[&str],
        query: &HashMap<String, String>,
    ) -> Result<ProviderSpec> {
        let [context_selector, namespace, kind, name, service_port] = segments else {
            bail!(
                "kubefwd URI must be /<context-selector>/<namespace>/<kind>/<name>/<service-port>"
            );
        };
        reject_unknown_query(query, &["local", "domain", "selector"])?;
        Ok(ProviderSpec::Kubefwd {
            cluster_endpoint: authority.to_string(),
            context_selector: (*context_selector).to_string(),
            namespace: namespace.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            service_port: service_port.parse::<u16>()?,
            domain: query.get("domain").cloned(),
            selector: query.get("selector").cloned(),
        })
    }
}
