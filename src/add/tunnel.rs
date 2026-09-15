use anyhow::{Result, bail};

use super::ask;
use super::cli::{pick_from_lines, run_cli_lines};
use super::require_or_ask;

pub fn tunnel_uri() -> Result<String> {
    let scheme = ask("Scheme (kubectl, tsh, ssh): ")?;
    match scheme.as_str() {
        "tsh" => {
            let _ = run_cli_lines("tsh", &["status"])?;
            require_or_ask(None, "Tunnel URI (tsh://…): ", true)
        }
        "kubectl" => pick_kubectl_tunnel(),
        _ => require_or_ask(None, "Tunnel URI: ", true),
    }
}

fn pick_kubectl_tunnel() -> Result<String> {
    let contexts = run_cli_lines("kubectl", &["config", "get-contexts", "-o", "name"])?;
    let context = pick_from_lines("kubectl contexts", &contexts)?;
    let namespaces = run_cli_lines(
        "kubectl",
        &["--context", &context, "get", "ns", "-o", "name"],
    )?;
    let ns = pick_from_lines("namespaces", &namespaces)?;
    let kind = ask("Kind (service, pod): ")?;
    let kind = if kind.is_empty() {
        "service"
    } else {
        kind.as_str()
    };
    let names = run_cli_lines(
        "kubectl",
        &["--context", &context, "-n", &ns, "get", kind, "-o", "name"],
    )?;
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
    Ok(format!(
        "kubectl://{host}/{context}/{ns}/{kind}/{name}/{port}"
    ))
}
