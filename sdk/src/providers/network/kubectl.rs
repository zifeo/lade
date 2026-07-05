use anyhow::{Result, bail};
use std::collections::HashMap;

use super::{NetworkProvider, ProviderSpec, reject_unknown_query};

pub struct KubectlProvider;

impl NetworkProvider for KubectlProvider {
    fn scheme(&self) -> &'static str {
        "kubectl"
    }

    fn parse(
        &self,
        authority: &str,
        segments: &[&str],
        query: &HashMap<String, String>,
    ) -> Result<ProviderSpec> {
        let [context_selector, namespace, kind, name, remote_port] = segments else {
            bail!(
                "kubectl URI must be /<context-selector>/<namespace>/<kind>/<name>/<remote-port>"
            );
        };
        reject_unknown_query(query, &["local", "pod-running-timeout"])?;
        Ok(ProviderSpec::Kubectl {
            cluster_endpoint: authority.to_string(),
            context_selector: (*context_selector).to_string(),
            namespace: namespace.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            remote_port: remote_port.to_string(),
            pod_running_timeout: query.get("pod-running-timeout").cloned(),
        })
    }
}
