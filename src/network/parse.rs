use anyhow::{Result, bail};
use lade_sdk::network::NetworkProviders;
use std::collections::HashMap;
use url::Url;

use crate::config::{NetworkBinding, is_valid_env_key};
use crate::network::types::{LocalTarget, ParsedBinding, ProviderSpec};

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
        .ok_or_else(|| anyhow::anyhow!("unsupported tunnel scheme '{}'", scheme))?;
    let spec = parser.parse(&authority, &segments, &query_map)?;
    // The app proxy speaks with the user's Teleport identity.
    if matches!(spec, ProviderSpec::TshApp { .. })
        && local_host != "127.0.0.1"
        && local_host != "localhost"
    {
        // § q6k9k1zr8.c@nzgmf
        bail!("tsh app proxy supports only 127.0.0.1 or localhost local hosts");
    }

    Ok(ParsedBinding {
        target,
        local_host,
        local_port,
        spec,
    })
}

/// `None`: an env-var key without `local=`. The claim takes a free port.
pub(crate) fn reconcile_local_port(
    target: &LocalTarget,
    uri_local: Option<u16>,
) -> Result<Option<u16>> {
    match target {
        LocalTarget::FixedPort(key_port) => {
            // § q6k9k1zr8.a@n15vr
            if let Some(uri_port) = uri_local
                && uri_port != *key_port
            {
                bail!(
                    "local port mismatch: key uses {}, URI local uses {}",
                    key_port,
                    uri_port
                );
            }
            Ok(Some(*key_port))
        }
        // § q6k9k1zr8.b@c7pdg
        LocalTarget::EnvVar(_) => Ok(uri_local),
    }
}

fn parse_target(key: &str) -> Result<LocalTarget> {
    if let Ok(port) = key.parse::<u16>() {
        // The kernel would pick another port while the tunnel records 0.
        // § q6k9k1zr8.a@c3mpn
        if port == 0 {
            bail!("invalid network binding key '0': a fixed local port is 1 to 65535");
        }
        return Ok(LocalTarget::FixedPort(port));
    }
    // § ps6s9115k@9k8wy
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
            if port == 0 {
                bail!("invalid local endpoint '{}': the port is 1 to 65535", value);
            }
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
            uri: "kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432"
                .to_string(),
            ttl: None,
        };
        let parsed = parse_binding(&binding).expect("parsed");
        assert!(matches!(parsed.target, LocalTarget::EnvVar(ref key) if key == "DB_PORT"));
    }

    #[test]
    fn parse_tsh_binding_with_local_query() {
        let binding = NetworkBinding {
            key: "15432".to_string(),
            uri: "tsh://teleport.example.com:443/kube_cluster/my-cluster/dev/service/postgres/5432?local=127.0.0.1:15432"
                .to_string(),
            ttl: None,
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
            uri:
                "kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432?bad=value"
                    .to_string(),
            ttl: None,
        };
        let err = parse_binding(&binding).expect_err("must reject unknown query");
        assert!(err.to_string().contains("unsupported query parameter"));
    }

    #[test]
    fn parse_binding_decodes_local_query_value() {
        let binding = NetworkBinding {
            key: "DB_PORT".to_string(),
            uri: "kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432?local=localhost%3A15432"
                .to_string(),
            ttl: None,
        };
        let parsed = parse_binding(&binding).expect("parsed");
        assert_eq!(parsed.local_host, "localhost");
        assert_eq!(parsed.local_port, Some(15432));
    }

    #[test]
    fn parse_binding_keeps_tsh_app_proxies_on_loopback() {
        let binding = NetworkBinding {
            key: "8080".to_string(),
            uri: "tsh://teleport.example.com:443/app/acme-api?local=0.0.0.0:8080".to_string(),
            ttl: None,
        };
        let err = parse_binding(&binding).expect_err("must reject host");
        assert!(err.to_string().contains("supports only"));
    }

    #[test]
    fn a_port_zero_key_or_local_fails_the_binding() {
        let uri = "kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432";
        let zero_key = NetworkBinding {
            key: "0".to_string(),
            uri: uri.to_string(),
            ttl: None,
        };
        let err = parse_binding(&zero_key).expect_err("key 0");
        assert!(err.to_string().contains("1 to 65535"), "{err:#}");
        let zero_local = NetworkBinding {
            key: "PGPORT".to_string(),
            uri: format!("{uri}?local=127.0.0.1:0"),
            ttl: None,
        };
        let err = parse_binding(&zero_local).expect_err("local port 0");
        assert!(err.to_string().contains("1 to 65535"), "{err:#}");
    }

    #[test]
    fn reconcile_refuses_a_local_port_that_differs_from_the_key() {
        let binding = NetworkBinding {
            key: "15432".to_string(),
            uri: "kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432?local=127.0.0.1:15433"
                .to_string(),
            ttl: None,
        };
        let parsed = parse_binding(&binding).expect("parsed");
        let err = reconcile_local_port(&parsed.target, parsed.local_port).expect_err("mismatch");
        assert!(err.to_string().contains("local port mismatch"), "{err:#}");
        assert_eq!(
            reconcile_local_port(&parsed.target, Some(15432)).unwrap(),
            Some(15432)
        );
    }
}
