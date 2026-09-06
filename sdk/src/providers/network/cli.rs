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
        min_version: "17.1.5",
        name: "Teleport tsh",
        install_url: "https://goteleport.com/docs/connect-your-client/tsh/",
    },
    NetworkCliSpec {
        scheme: "ssh",
        bin: "ssh",
        version_args: &["-V"],
        min_version: "7.6.0",
        name: "OpenSSH",
        install_url: "https://www.openssh.com/",
    },
];

pub fn is_network_scheme(scheme: &str) -> bool {
    NETWORK_CLI_SPECS.iter().any(|spec| spec.scheme == scheme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::network::NetworkProviders;
    use semver::Version;

    #[test]
    fn specs_cover_registered_providers() {
        let providers = NetworkProviders::new();
        for spec in NETWORK_CLI_SPECS {
            assert!(
                Version::parse(spec.min_version).is_ok(),
                "{} has invalid min_version {}",
                spec.scheme,
                spec.min_version
            );
            assert!(
                providers.provider(spec.scheme).is_some(),
                "NETWORK_CLI_SPECS has '{}', but NetworkProviders does not",
                spec.scheme
            );
        }
        for scheme in ["kubectl", "kubefwd", "tsh", "ssh"] {
            assert!(
                is_network_scheme(scheme),
                "{scheme} must be a network scheme"
            );
        }
        assert!(!is_network_scheme("op"));
        assert!(!is_network_scheme("file"));
    }
}
