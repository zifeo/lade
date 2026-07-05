use anyhow::{Result, bail};
use std::process::Command;

use crate::network::kubeconfig::resolve_kube_context;
use crate::network::types::ProviderSpec;

pub(crate) fn build_command(
    spec: &ProviderSpec,
    local_host: &str,
    local_port: u16,
) -> Result<Command> {
    match spec {
        ProviderSpec::Kubectl {
            cluster_endpoint,
            context_selector,
            namespace,
            kind,
            name,
            remote_port,
            pod_running_timeout,
        } => {
            let context = resolve_kube_context(cluster_endpoint, Some(context_selector.as_str()))?;
            let mut cmd = Command::new("kubectl");
            cmd.arg("--context")
                .arg(context)
                .arg("-n")
                .arg(namespace)
                .arg("port-forward")
                .arg(format!("{kind}/{name}"))
                .arg(format!("{local_port}:{remote_port}"))
                .arg("--address")
                .arg(local_host);
            if let Some(timeout) = pod_running_timeout {
                cmd.arg("--pod-running-timeout").arg(timeout);
            }
            Ok(cmd)
        }
        ProviderSpec::Kubefwd {
            cluster_endpoint,
            context_selector,
            namespace,
            kind,
            name,
            service_port,
            domain,
            selector,
        } => {
            if kind != "service" {
                bail!("kubefwd currently supports only resource kind 'service'");
            }
            let context = resolve_kube_context(cluster_endpoint, Some(context_selector.as_str()))?;
            let mut cmd = Command::new("kubefwd");
            cmd.arg("svc")
                .arg("-n")
                .arg(namespace)
                .arg("-x")
                .arg(context)
                .arg("-m")
                .arg(format!("{service_port}:{local_port}"))
                .arg("-f")
                .arg(format!("metadata.name={name}"));
            if let Some(domain) = domain {
                cmd.arg("-d").arg(domain);
            }
            if let Some(selector) = selector {
                cmd.arg("-l").arg(selector);
            }
            Ok(cmd)
        }
        ProviderSpec::Tsh {
            teleport_proxy,
            namespace,
            kind,
            name,
            remote_port,
            ..
        } => {
            let mut cmd = Command::new("tsh");
            cmd.arg(format!("--proxy={teleport_proxy}"))
                .arg("kubectl")
                .arg("-n")
                .arg(namespace)
                .arg("port-forward")
                .arg(format!("{kind}/{name}"))
                .arg(format!("{local_port}:{remote_port}"))
                .arg("--address")
                .arg(local_host);
            Ok(cmd)
        }
        ProviderSpec::Ssh {
            jump_host,
            jump_port,
            remote_host,
            remote_port,
        } => {
            let mut cmd = Command::new("ssh");
            cmd.arg("-N")
                .arg("-o")
                .arg("ExitOnForwardFailure=yes")
                .arg("-L")
                .arg(format!(
                    "{local_host}:{local_port}:{remote_host}:{remote_port}"
                ))
                .arg("-p")
                .arg(jump_port.to_string())
                .arg(jump_host);
            Ok(cmd)
        }
    }
}

pub(crate) fn ensure_provider_preflight(spec: &ProviderSpec) -> Result<()> {
    if let ProviderSpec::Tsh {
        teleport_proxy,
        kube_cluster,
        ..
    } = spec
    {
        let status = Command::new("tsh")
            .arg(format!("--proxy={teleport_proxy}"))
            .arg("kube")
            .arg("login")
            .arg(kube_cluster)
            .status()?;
        if !status.success() {
            bail!("tsh kube login failed for cluster '{}'", kube_cluster);
        }
    }
    if let ProviderSpec::Ssh { .. } = spec {
        // No preflight for SSH
    }
    Ok(())
}
