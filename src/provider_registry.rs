use lade_sdk::compat::CLI_SPECS as SECRET_CLI_SPECS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Secret,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkCliSpec {
    pub scheme: &'static str,
    pub bin: &'static str,
    pub version_args: &'static [&'static str],
    pub min_version: &'static str,
    pub name: &'static str,
    pub install_url: &'static str,
}

pub const NETWORK_CLI_SPECS: &[NetworkCliSpec] = &[
    NetworkCliSpec {
        scheme: "kubectl",
        bin: "kubectl",
        version_args: &["version", "--client", "--output=json"],
        min_version: "1.27.0",
        name: "kubectl",
        install_url: "https://kubernetes.io/docs/tasks/tools/",
    },
    NetworkCliSpec {
        scheme: "kubefwd",
        bin: "kubefwd",
        version_args: &["version"],
        min_version: "1.22.0",
        name: "kubefwd",
        install_url: "https://github.com/txn2/kubefwd",
    },
    NetworkCliSpec {
        scheme: "tsh",
        bin: "tsh",
        version_args: &["version"],
        min_version: "17.0.0",
        name: "Teleport tsh",
        install_url: "https://goteleport.com/docs/connect-your-client/tsh/",
    },
];

pub fn provider_kind_for_scheme(scheme: &str) -> Option<ProviderKind> {
    if SECRET_CLI_SPECS.iter().any(|spec| spec.scheme == scheme) {
        return Some(ProviderKind::Secret);
    }
    if NETWORK_CLI_SPECS.iter().any(|spec| spec.scheme == scheme) {
        return Some(ProviderKind::Network);
    }
    None
}

pub fn is_network_scheme(scheme: &str) -> bool {
    provider_kind_for_scheme(scheme) == Some(ProviderKind::Network)
}

pub fn all_supported_schemes() -> Vec<String> {
    let mut out = Vec::new();
    for spec in SECRET_CLI_SPECS {
        out.push(spec.scheme.to_string());
    }
    for spec in NETWORK_CLI_SPECS {
        out.push(spec.scheme.to_string());
    }
    out
}
