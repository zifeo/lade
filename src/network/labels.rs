use std::time::Instant;

use crate::network::progress::{ProviderProgressEvent, ProviderProgressKind, format_timing};
use crate::network::types::{LocalTarget, ProviderSpec};
use crate::provider_progress::ProviderProgressSink;

pub(super) fn env_entry_for(target: &LocalTarget, local_port: u16) -> Option<(String, String)> {
    match target {
        LocalTarget::EnvVar(name) => Some((name.clone(), local_port.to_string())),
        LocalTarget::FixedPort(_) => None,
    }
}

pub(super) fn connection_label(spec: &ProviderSpec, local_host: &str, local_port: u16) -> String {
    let local = if local_host == "127.0.0.1" || local_host == "localhost" {
        local_port.to_string()
    } else {
        format!("{local_host}:{local_port}")
    };
    match spec {
        ProviderSpec::Kubectl {
            name, remote_port, ..
        } => format!("{name}:{remote_port} on {local}"),
        ProviderSpec::Kubefwd {
            name, service_port, ..
        } => format!("{name}:{service_port} on {local}"),
        ProviderSpec::TshKubeCluster {
            name, remote_port, ..
        } => format!("{name}:{remote_port} on {local}"),
        ProviderSpec::TshApp {
            app_name,
            target_port,
            ..
        } => match target_port {
            Some(target_port) => format!("{app_name}:{target_port} on {local}"),
            None => format!("{app_name} on {local}"),
        },
        ProviderSpec::Ssh {
            remote_host,
            remote_port,
            ..
        } => format!("{remote_host}:{remote_port} on {local}"),
    }
}

pub(super) fn provider_label(spec: &ProviderSpec) -> &'static str {
    match spec {
        ProviderSpec::Kubectl { .. } => "kubectl forward",
        ProviderSpec::Kubefwd { .. } => "kubefwd forward",
        ProviderSpec::TshKubeCluster { .. } => "tsh kube_cluster forward",
        ProviderSpec::TshApp { .. } => "tsh app proxy",
        ProviderSpec::Ssh { .. } => "ssh forward",
    }
}

pub(super) fn send_failed(
    progress: &ProviderProgressSink,
    id: String,
    display: String,
    started: Instant,
) {
    send_progress(
        progress,
        &id,
        format_timing(&display, started),
        ProviderProgressKind::Failed,
    );
}

pub(super) fn send_progress(
    progress: &ProviderProgressSink,
    id: &str,
    display: String,
    kind: ProviderProgressKind,
) {
    progress.send(ProviderProgressEvent {
        id: id.to_string(),
        display,
        kind,
    });
}
