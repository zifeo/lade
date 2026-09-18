use anyhow::{Result, bail};
use std::collections::HashMap;

use super::{NetworkProvider, ProviderSpec, reject_unknown_query};

pub struct KubectlProvider;

impl NetworkProvider for KubectlProvider {
    fn scheme(&self) -> &'static str {
        "kubectl"
    }

    fn parse(
        &self,
        authority: &str,
        segments: &[&str],
        query: &HashMap<String, String>,
    ) -> Result<ProviderSpec> {
        let [context_selector, namespace, kind, name, remote_port] = segments else {
            bail!(
                "kubectl URI must be /<context-selector>/<namespace>/<kind>/<name>/<remote-port>"
            );
        };
        reject_unknown_query(query, &["local", "pod-running-timeout"])?;
        Ok(ProviderSpec::Kubectl {
            cluster_endpoint: authority.to_string(),
            context_selector: (*context_selector).to_string(),
            namespace: namespace.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            remote_port: remote_port.to_string(),
            pod_running_timeout: query.get("pod-running-timeout").cloned(),
        })
    }

    fn search(&self, extra_env: &HashMap<String, String>) -> Result<Vec<String>> {
        kubectl_lines(extra_env, &["config", "get-contexts", "-o", "name"])
    }

    fn list(
        &self,
        extra_env: &HashMap<String, String>,
        what: &str,
        args: &[&str],
    ) -> Result<Vec<String>> {
        match (what, args) {
            ("namespaces", [context]) => kubectl_lines(
                extra_env,
                &["--context", context, "get", "ns", "-o", "name"],
            ),
            ("resources", [context, namespace, kind]) => kubectl_lines(
                extra_env,
                &[
                    "--context",
                    context,
                    "-n",
                    namespace,
                    "get",
                    kind,
                    "-o",
                    "name",
                ],
            ),
            _ => bail!("kubectl list {what} needs the right args"),
        }
    }
}

fn kubectl_lines(extra_env: &HashMap<String, String>, args: &[&str]) -> Result<Vec<String>> {
    let output = std::process::Command::new("kubectl")
        .args(args)
        .envs(extra_env)
        .output()?;
    if !output.status.success() {
        bail!("kubectl login required");
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_start_matches("namespace/").to_string())
        .map(|line| line.split('/').next_back().unwrap_or(&line).to_string())
        .collect())
}

pub fn compose_uri(
    host: &str,
    context: &str,
    namespace: &str,
    kind: &str,
    name: &str,
    port: &str,
) -> String {
    format!("kubectl://{host}/{context}/{namespace}/{kind}/{name}/{port}")
}
