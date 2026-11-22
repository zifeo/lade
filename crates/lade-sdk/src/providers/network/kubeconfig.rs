use anyhow::{Result, bail};
use serde::Deserialize;
use std::collections::HashMap;

/// Resolves a kube context for `cluster_endpoint`. `kubectl config view` is a
/// local, read-only, idempotent operation (no network call, no write), so
/// this is safe to call concurrently from multiple threads without any
/// shared cache or lock: caching would only save a cheap subprocess call at
/// the cost of serializing unrelated bindings against each other.
pub(crate) fn resolve_kube_context(
    cluster_endpoint: &str,
    selector: Option<&str>,
) -> Result<String> {
    let output = crate::providers::search_cli(
        "kubectl",
        &["config", "view", "-o", "json"],
        &HashMap::new(),
        crate::providers::PROVIDER_TIMEOUT,
    )?;
    if !output.status.success() {
        bail!(
            "could not resolve kube context for '{}': {}",
            cluster_endpoint,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    context_in_view(&output.stdout, cluster_endpoint, selector)
}

/// The context of `kubectl config view -o json` that ties to
/// `cluster_endpoint`: `selector` when it does, the only one otherwise.
fn context_in_view(view: &[u8], cluster_endpoint: &str, selector: Option<&str>) -> Result<String> {
    let config: KubeConfig = serde_json::from_slice(view)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: &[u8] = br#"{
        "clusters": [
            {"name": "prd", "cluster": {"server": "https://prd.example.com"}},
            {"name": "stg", "cluster": {"server": "https://stg.example.com:6443"}}
        ],
        "contexts": [
            {"name": "prd-admin", "context": {"cluster": "prd"}},
            {"name": "stg-admin", "context": {"cluster": "stg"}}
        ]
    }"#;

    #[test]
    fn a_context_tied_to_another_endpoint_fails() {
        let err = context_in_view(VIEW, "prd.example.com:443", Some("stg-admin")).unwrap_err();
        assert!(
            err.to_string().contains("does not match endpoint"),
            "{err:#}"
        );
    }

    #[test]
    fn a_context_tied_to_the_endpoint_is_returned() {
        assert_eq!(
            context_in_view(VIEW, "prd.example.com:443", Some("prd-admin")).unwrap(),
            "prd-admin"
        );
        assert_eq!(
            context_in_view(VIEW, "stg.example.com:6443", None).unwrap(),
            "stg-admin"
        );
        assert!(context_in_view(VIEW, "dev.example.com:443", None).is_err());
    }

    #[test]
    fn an_https_endpoint_without_a_port_is_443() {
        assert_eq!(
            normalize_cluster_endpoint("https://h").as_deref(),
            Some("h:443")
        );
        assert_eq!(
            normalize_cluster_endpoint("https://h:6443/path").as_deref(),
            Some("h:6443")
        );
    }
}
