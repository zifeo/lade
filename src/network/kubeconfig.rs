use anyhow::{Result, bail};
use serde::Deserialize;
use std::process::Command;

/// Resolves a kube context for `cluster_endpoint`. `kubectl config view` is a
/// local, read-only, idempotent operation (no network call, no write), so
/// this is safe to call concurrently from multiple threads without any
/// shared cache or lock: caching would only save a cheap subprocess call at
/// the cost of serializing unrelated bindings against each other.
pub(crate) fn resolve_kube_context(
    cluster_endpoint: &str,
    selector: Option<&str>,
) -> Result<String> {
    let output = Command::new("kubectl")
        .args(["config", "view", "-o", "json"])
        .output()?;
    if !output.status.success() {
        bail!(
            "could not resolve kube context for '{}': {}",
            cluster_endpoint,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let config: KubeConfig = serde_json::from_slice(&output.stdout)?;
    let clusters = config
        .clusters
        .unwrap_or_default()
        .into_iter()
        .filter_map(|cluster| {
            let server = cluster.cluster.server?;
            (normalize_cluster_endpoint(&server).as_deref() == Some(cluster_endpoint))
                .then_some(cluster.name)
        })
        .collect::<Vec<_>>();
    if clusters.is_empty() {
        bail!(
            "no kubeconfig cluster matches endpoint '{}'",
            cluster_endpoint
        );
    }
    let matching_contexts = config
        .contexts
        .unwrap_or_default()
        .into_iter()
        .filter(|ctx| clusters.contains(&ctx.context.cluster))
        .map(|ctx| ctx.name)
        .collect::<Vec<_>>();
    if let Some(selected) = selector {
        if !matching_contexts.iter().any(|ctx| ctx == selected) {
            bail!(
                "selected kube context '{}' does not match endpoint '{}' (available: {})",
                selected,
                cluster_endpoint,
                matching_contexts.join(", ")
            );
        }
        return Ok(selected.to_string());
    }
    match matching_contexts.as_slice() {
        [only] => Ok(only.clone()),
        [] => bail!(
            "no kubeconfig context references endpoint '{}'",
            cluster_endpoint
        ),
        _ => bail!(
            "ambiguous kubeconfig contexts for endpoint '{}': {}",
            cluster_endpoint,
            matching_contexts.join(", ")
        ),
    }
}

fn normalize_cluster_endpoint(server: &str) -> Option<String> {
    let (scheme, rest) = match server.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, server),
    };
    let authority = rest.split('/').next()?;
    if authority.contains(':') {
        return Some(authority.to_string());
    }
    match scheme {
        Some("https") => Some(format!("{authority}:443")),
        Some("http") => Some(format!("{authority}:80")),
        _ => Some(authority.to_string()),
    }
}

#[derive(Deserialize)]
struct KubeConfig {
    #[serde(default)]
    clusters: Option<Vec<NamedCluster>>,
    #[serde(default)]
    contexts: Option<Vec<NamedContext>>,
}

#[derive(Deserialize)]
struct NamedCluster {
    name: String,
    cluster: ClusterInner,
}

#[derive(Deserialize)]
struct ClusterInner {
    server: Option<String>,
}

#[derive(Deserialize)]
struct NamedContext {
    name: String,
    context: ContextInner,
}

#[derive(Deserialize)]
struct ContextInner {
    cluster: String,
}
