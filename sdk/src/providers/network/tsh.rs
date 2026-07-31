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
        reject_unknown_query(query, &["local"])?;
        match segments {
            ["app", app_name] => Ok(ProviderSpec::TshApp {
                teleport_proxy: authority.to_string(),
                app_name: (*app_name).to_string(),
                target_port: None,
            }),
            ["app", app_name, target_port] => Ok(ProviderSpec::TshApp {
                teleport_proxy: authority.to_string(),
                app_name: (*app_name).to_string(),
                target_port: Some(target_port.parse::<u16>()?),
            }),
            [
                "kube_cluster",
                kube_cluster,
                namespace,
                kind,
                name,
                remote_port,
            ] => Ok(ProviderSpec::TshKubeCluster {
                teleport_proxy: authority.to_string(),
                kube_cluster: (*kube_cluster).to_string(),
                namespace: (*namespace).to_string(),
                kind: (*kind).to_string(),
                name: (*name).to_string(),
                remote_port: remote_port.parse::<u16>()?,
            }),
            _ => bail!(
                "tsh URI must be /app/<app-name>[/<target-port>] or /kube_cluster/<kube-cluster>/<namespace>/<kind>/<name>/<remote-port>"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_app() {
        let spec = TshProvider
            .parse(
                "teleport.example.com:443",
                &["app", "example-app"],
                &HashMap::new(),
            )
            .expect("parsed");

        assert!(matches!(
            spec,
            ProviderSpec::TshApp {
                app_name,
                target_port: None,
                ..
            } if app_name == "example-app"
        ));
    }

    #[test]
    fn parses_app_target_port() {
        let spec = TshProvider
            .parse(
                "teleport.example.com:443",
                &["app", "example-app", "3000"],
                &HashMap::new(),
            )
            .expect("parsed");

        assert!(matches!(
            spec,
            ProviderSpec::TshApp {
                app_name,
                target_port: Some(3000),
                ..
            } if app_name == "example-app"
        ));
    }

    #[test]
    fn parses_kube_cluster() {
        let spec = TshProvider
            .parse(
                "teleport.example.com:443",
                &[
                    "kube_cluster",
                    "prod",
                    "monitoring",
                    "service",
                    "grafana",
                    "3000",
                ],
                &HashMap::new(),
            )
            .expect("parsed");

        assert!(matches!(
            spec,
            ProviderSpec::TshKubeCluster {
                kube_cluster,
                namespace,
                ..
            } if kube_cluster == "prod" && namespace == "monitoring"
        ));
    }
}
