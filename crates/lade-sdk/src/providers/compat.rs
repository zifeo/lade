use super::{Providers, Transport};

pub struct CliSpec {
    pub scheme: &'static str,
    pub bin: &'static str,
    pub min_version: &'static str,
    pub max_version: Option<&'static str>,
    pub mise: Option<&'static str>,
    pub tunnel: bool,
    pub docs: Option<&'static str>,
}

pub static CLI_SPECS: &[CliSpec] = &[
    CliSpec {
        scheme: "op",
        bin: "op",
        min_version: "2.18.0",
        max_version: None,
        mise: Some("aqua/1password/op"),
        tunnel: false,
        docs: Some("https://developer.1password.com/docs/cli/get-started/"),
    },
    CliSpec {
        scheme: "doppler",
        bin: "doppler",
        min_version: "3.76.0",
        max_version: None,
        mise: Some("aqua/DopplerHQ/cli"),
        tunnel: false,
        docs: Some("https://docs.doppler.com/docs/cli"),
    },
    CliSpec {
        scheme: "passbolt",
        bin: "passbolt",
        min_version: "0.5.0",
        max_version: None,
        mise: Some("github/passbolt/go-passbolt-cli"),
        tunnel: false,
        docs: Some("https://www.passbolt.com/docs/user-guide/cli/"),
    },
    CliSpec {
        scheme: "sops",
        bin: "sops",
        min_version: "3.8.0",
        max_version: None,
        mise: Some("aqua/getsops/sops"),
        tunnel: false,
        docs: Some("https://github.com/getsops/sops"),
    },
    CliSpec {
        scheme: "infisical",
        bin: "infisical",
        min_version: "0.4.0",
        max_version: None,
        mise: Some("aqua/Infisical/infisical"),
        tunnel: false,
        docs: Some("https://infisical.com/docs/cli/overview"),
    },
    CliSpec {
        scheme: "bw",
        bin: "bw",
        min_version: "2023.1.0",
        max_version: None,
        mise: Some("aqua/bitwarden/clients"),
        tunnel: false,
        docs: Some("https://bitwarden.com/help/cli/"),
    },
    CliSpec {
        scheme: "vault",
        bin: "vault",
        min_version: "1.15.0",
        max_version: None,
        // Hydrate is HTTP KV v2. vault login still writes the token
        // file. No implied mise pin.
        mise: None,
        tunnel: false,
        docs: Some("https://developer.hashicorp.com/vault/docs/commands/login"),
    },
    CliSpec {
        scheme: "awssm",
        bin: "aws",
        min_version: "2.15.0",
        max_version: None,
        mise: Some("aqua/aws/aws-cli"),
        tunnel: false,
        docs: Some("https://docs.aws.amazon.com/cli/latest/userguide/cli-chap-configure.html"),
    },
    CliSpec {
        scheme: "azurekv",
        bin: "az",
        min_version: "2.50.0",
        max_version: None,
        mise: Some("aqua/Azure/azure-cli"),
        tunnel: false,
        docs: Some("https://learn.microsoft.com/en-us/cli/azure/authenticate-azure-cli"),
    },
    CliSpec {
        scheme: "gcpsm",
        bin: "gcloud",
        min_version: "450.0.0",
        max_version: None,
        mise: Some("aqua/GoogleCloudPlatform/cloud-sdk"),
        tunnel: false,
        docs: Some("https://cloud.google.com/sdk/docs/authorizing"),
    },
    CliSpec {
        scheme: "kubectl",
        bin: "kubectl",
        min_version: "1.27.0",
        max_version: None,
        mise: Some("aqua/kubernetes/kubectl"),
        tunnel: true,
        docs: Some("https://kubernetes.io/docs/reference/access-authn-authz/authentication/"),
    },
    CliSpec {
        scheme: "kubefwd",
        bin: "kubefwd",
        min_version: "1.22.0",
        max_version: None,
        mise: Some("aqua/txn2/kubefwd"),
        tunnel: true,
        docs: Some("https://github.com/txn2/kubefwd"),
    },
    CliSpec {
        scheme: "tsh",
        bin: "tsh",
        min_version: "17.1.5",
        max_version: None,
        mise: Some("aqua/gravitational/teleport"),
        tunnel: true,
        docs: Some("https://goteleport.com/docs/connect-your-client/tsh/"),
    },
    CliSpec {
        scheme: "ssh",
        bin: "ssh",
        min_version: "7.6.0",
        max_version: None,
        // Ambient OpenSSH. No mise pin: there is no portable registry
        // build we trust. Missing ssh is a box, not a fetch.
        mise: None,
        tunnel: true,
        docs: Some("https://www.openssh.com/"),
    },
    CliSpec {
        scheme: "apm",
        bin: "apm",
        min_version: "0.23.1",
        max_version: None,
        mise: Some("github/microsoft/apm"),
        tunnel: false,
        docs: Some("https://github.com/microsoft/apm"),
    },
    CliSpec {
        scheme: "skills",
        bin: "skills",
        min_version: "1.4.0",
        max_version: None,
        mise: Some("npm/skills"),
        tunnel: false,
        docs: Some("https://github.com/vercel-labs/skills"),
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkSpec {
    pub scheme: &'static str,
    pub version: &'static str,
}

pub fn sdk_specs() -> Vec<SdkSpec> {
    let providers = Providers::new();
    providers
        .registered_schemes()
        .into_iter()
        .filter_map(|scheme| {
            let provider = providers.provider(scheme)?;
            (provider.transport() == Transport::Sdk).then_some(SdkSpec {
                scheme,
                version: env!("CARGO_PKG_VERSION"),
            })
        })
        .collect()
}

pub fn spec_for(scheme: &str) -> Option<&'static CliSpec> {
    CLI_SPECS.iter().find(|s| s.scheme == scheme)
}

pub fn spec_for_bin(bin: &str) -> Option<&'static CliSpec> {
    CLI_SPECS.iter().find(|s| s.bin == bin)
}

pub fn is_network_scheme(scheme: &str) -> bool {
    spec_for(scheme).is_some_and(|spec| spec.tunnel)
}

pub fn all_supported_schemes() -> Vec<String> {
    let mut out = Vec::new();
    for scheme in Providers::new().registered_schemes() {
        out.push(scheme.to_string());
    }
    for spec in CLI_SPECS.iter().filter(|spec| spec.tunnel) {
        out.push(spec.scheme.to_string());
    }
    out
}

pub fn is_secret_scheme(scheme: &str) -> bool {
    Providers::new().provider(scheme).is_some()
}

#[cfg(test)]
mod tests;
