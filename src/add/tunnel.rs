use anyhow::{Context, Result, bail};
use lade_sdk::network::NetworkProviders;

use super::ask;
use super::cli::{locked_path_env, login_stop, pick_from_lines};
use super::require_or_ask;

pub fn tunnel_uri() -> Result<String> {
    let scheme = ask("Scheme (kubectl, tsh, ssh): ")?;
    match scheme.as_str() {
        "tsh" => {
            let env = locked_path_env("tsh")?;
            let providers = NetworkProviders::new();
            let provider = providers.provider("tsh").context("tsh provider")?;
            if provider.search(&env).is_err() {
                return login_stop("tsh");
            }
            require_or_ask(None, "Tunnel URI (tsh://…): ", true)
        }
        "kubectl" => pick_kubectl_tunnel(),
        _ => require_or_ask(None, "Tunnel URI: ", true),
    }
}

fn pick_kubectl_tunnel() -> Result<String> {
    let env = locked_path_env("kubectl")?;
    let providers = NetworkProviders::new();
    let provider = providers.provider("kubectl").context("kubectl provider")?;
    let contexts = match provider.search(&env) {
        Ok(hits) if !hits.is_empty() => hits,
        _ => return login_stop("kubectl"),
    };
    let context = pick_from_lines("kubectl contexts", &contexts)?;
    let namespaces = match provider.list(&env, "namespaces", &[&context]) {
        Ok(hits) if !hits.is_empty() => hits,
        _ => return login_stop("kubectl"),
    };
    let ns = pick_from_lines("namespaces", &namespaces)?;
    let kind = ask("Kind (service, pod): ")?;
    let kind = if kind.is_empty() {
        "service"
    } else {
        kind.as_str()
    };
    let names = match provider.list(&env, "resources", &[&context, &ns, kind]) {
        Ok(hits) if !hits.is_empty() => hits,
        _ => return login_stop("kubectl"),
    };
    let name = pick_from_lines(&format!("{kind}s"), &names)?;
    let port = ask("Remote port: ")?;
    let host = ask("API host:port (127.0.0.1:6443): ")?;
    let host = if host.is_empty() {
        "127.0.0.1:6443"
    } else {
        host.as_str()
    };
    if port.is_empty() {
        bail!("port is required");
    }
    Ok(lade_sdk::network::compose_kubectl_uri(
        host, &context, &ns, kind, &name, &port,
    ))
}
