use anyhow::{Result, bail};
use lade_sdk::network::NetworkProviders;
use std::collections::HashMap;
use std::net::TcpListener;
use url::Url;

use crate::config::{NetworkBinding, is_valid_env_key};
use crate::network::types::{LocalTarget, ParsedBinding};

pub(crate) fn parse_binding(binding: &NetworkBinding) -> Result<ParsedBinding> {
    let target = parse_target(&binding.key)?;
    let url = Url::parse(&binding.uri)
        .map_err(|e| anyhow::anyhow!("invalid URI '{}': {}", binding.uri, e))?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("invalid URI '{}': user info is not supported", binding.uri);
    }
    let scheme = url.scheme();
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("invalid URI '{}': missing authority", binding.uri))?;
    let authority = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    let query_map = parse_query(&url);
    let segments = parse_path_segments(&url, &binding.uri)?;

    let (local_host, local_port) =
        parse_local_endpoint(query_map.get("local").map(String::as_str))?;
    let providers = NetworkProviders::new();
    let parser = providers
        .provider(scheme)
        .ok_or_else(|| anyhow::anyhow!("unsupported network provider scheme '{}'", scheme))?;
    let spec = parser.parse(&authority, &segments, &query_map)?;

    Ok(ParsedBinding {
        target,
        local_host,
        local_port,
        spec,
        source_uri: binding.uri.clone(),
    })
}

pub(crate) fn reconcile_local_port(
    target: &LocalTarget,
    uri_local: Option<u16>,
    local_host: &str,
) -> Result<u16> {
    match target {
        LocalTarget::FixedPort(key_port) => {
            if let Some(uri_port) = uri_local
                && uri_port != *key_port
            {
                bail!(
                    "local port mismatch: key uses {}, URI local uses {}",
                    key_port,
                    uri_port
                );
            }
            Ok(*key_port)
        }
        LocalTarget::EnvVar(_) => match uri_local {
            Some(port) => Ok(port),
            None => pick_free_local_port(local_host),
        },
    }
}

fn pick_free_local_port(host: &str) -> Result<u16> {
    let bind_host = if host == "localhost" {
        "127.0.0.1"
    } else {
        host
    };
    let listener = TcpListener::bind((bind_host, 0))?;
    Ok(listener.local_addr()?.port())
}

fn parse_target(key: &str) -> Result<LocalTarget> {
    if let Ok(port) = key.parse::<u16>() {
        return Ok(LocalTarget::FixedPort(port));
    }
    if !is_valid_env_key(key) {
        bail!(
            "invalid network binding key '{}': use a local port number or a valid env var name",
            key
        );
    }
    Ok(LocalTarget::EnvVar(key.to_string()))
}

fn parse_path_segments<'a>(url: &'a Url, raw_uri: &str) -> Result<Vec<&'a str>> {
    let Some(segments) = url.path_segments() else {
        bail!("invalid URI '{}': missing path", raw_uri);
    };
    let collected = segments.collect::<Vec<_>>();
    if collected.is_empty() || collected.iter().any(|segment| segment.is_empty()) {
        bail!("invalid URI '{}': missing path", raw_uri);
    }
    Ok(collected)
}

fn parse_query(url: &Url) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (key, value) in url.query_pairs() {
        out.insert(key.into_owned(), value.into_owned());
    }
    out
}

fn parse_local_endpoint(local: Option<&str>) -> Result<(String, Option<u16>)> {
    match local {
        None => Ok(("127.0.0.1".to_string(), None)),
        Some(value) => {
            let (host, port) = value
                .rsplit_once(':')
                .ok_or_else(|| anyhow::anyhow!("invalid local endpoint '{}'", value))?;
            if host.is_empty() {
                bail!("invalid local endpoint '{}': missing host", value);
            }
            let port = port.parse::<u16>()?;
            Ok((host.to_string(), Some(port)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kubectl_binding_with_env_key() {
        let binding = NetworkBinding {
            key: "DB_PORT".to_string(),
            uri: "kubectl://k8s.example.com:6443/claryo-gcp-01/dev/service/postgres/5432"
                .to_string(),
        };
        let parsed = parse_binding(&binding).expect("parsed");
        assert!(matches!(parsed.target, LocalTarget::EnvVar(ref key) if key == "DB_PORT"));
    }

    #[test]
    fn parse_tsh_binding_with_local_query() {
        let binding = NetworkBinding {
            key: "15432".to_string(),
            uri: "tsh://teleport.example.com:443/my-cluster/dev/service/postgres/5432?local=127.0.0.1:15432"
                .to_string(),
        };
        let parsed = parse_binding(&binding).expect("parsed");
        assert!(matches!(parsed.target, LocalTarget::FixedPort(15432)));
        assert_eq!(parsed.local_host, "127.0.0.1");
        assert_eq!(parsed.local_port, Some(15432));
    }

    #[test]
    fn parse_binding_rejects_unknown_query_parameter() {
        let binding = NetworkBinding {
            key: "DB_PORT".to_string(),
            uri: "kubectl://k8s.example.com:6443/claryo-gcp-01/dev/service/postgres/5432?bad=value"
                .to_string(),
        };
        let err = parse_binding(&binding).expect_err("must reject unknown query");
        assert!(err.to_string().contains("unsupported query parameter"));
    }

    #[test]
    fn parse_binding_decodes_local_query_value() {
        let binding = NetworkBinding {
            key: "DB_PORT".to_string(),
            uri: "kubectl://k8s.example.com:6443/claryo-gcp-01/dev/service/postgres/5432?local=localhost%3A15432"
                .to_string(),
        };
        let parsed = parse_binding(&binding).expect("parsed");
        assert_eq!(parsed.local_host, "localhost");
        assert_eq!(parsed.local_port, Some(15432));
    }
}
