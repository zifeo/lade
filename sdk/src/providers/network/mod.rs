use anyhow::{Result, bail};
use rustc_hash::FxHashMap;
use std::collections::HashMap;

mod kubectl;
mod kubefwd;
mod ssh;
mod tsh;

#[derive(Debug, Clone)]
pub enum ProviderSpec {
    Kubectl {
        cluster_endpoint: String,
        context_selector: String,
        namespace: String,
        kind: String,
        name: String,
        remote_port: String,
        pod_running_timeout: Option<String>,
    },
    Kubefwd {
        cluster_endpoint: String,
        context_selector: String,
        namespace: String,
        kind: String,
        name: String,
        service_port: u16,
        domain: Option<String>,
        selector: Option<String>,
    },
    Tsh {
        teleport_proxy: String,
        kube_cluster: String,
        namespace: String,
        kind: String,
        name: String,
        remote_port: u16,
    },
    Ssh {
        jump_host: String,
        jump_port: u16,
        remote_host: String,
        remote_port: u16,
    },
}

pub trait NetworkProvider: Sync {
    fn scheme(&self) -> &'static str;
    fn parse(
        &self,
        authority: &str,
        segments: &[&str],
        query: &HashMap<String, String>,
    ) -> Result<ProviderSpec>;
}

pub struct NetworkProviders {
    by_scheme: FxHashMap<&'static str, Box<dyn NetworkProvider + Send>>,
}

impl Default for NetworkProviders {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkProviders {
    pub fn new() -> Self {
        let providers: Vec<Box<dyn NetworkProvider + Send>> = vec![
            Box::new(kubectl::KubectlProvider),
            Box::new(kubefwd::KubefwdProvider),
            Box::new(tsh::TshProvider),
            Box::new(ssh::SshProvider),
        ];
        let by_scheme = providers
            .into_iter()
            .map(|provider| (provider.scheme(), provider))
            .collect();
        Self { by_scheme }
    }

    pub fn provider(&self, scheme: &str) -> Option<&(dyn NetworkProvider + Send)> {
        self.by_scheme.get(scheme).map(|p| p.as_ref())
    }
}

pub(crate) fn reject_unknown_query(
    query: &HashMap<String, String>,
    allowed: &[&str],
) -> Result<()> {
    for key in query.keys() {
        if !allowed.contains(&key.as_str()) {
            bail!("unsupported query parameter '{}'", key);
        }
    }
    Ok(())
}
