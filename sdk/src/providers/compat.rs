use std::collections::HashMap;

use log::debug;
use regex::Regex;
use semver::Version;

use super::network::NETWORK_CLI_SPECS;
use super::{Providers, Transport, run_cli};

pub struct CliSpec {
    pub scheme: &'static str,
    pub bin: &'static str,
    pub version_args: &'static [&'static str],
    pub min_version: &'static str,
}

pub static CLI_SPECS: &[CliSpec] = &[
    CliSpec {
        scheme: "op",
        bin: "op",
        version_args: &["--version"],
        min_version: "2.18.0",
    },
    CliSpec {
        scheme: "doppler",
        bin: "doppler",
        version_args: &["--version"],
        min_version: "3.76.0",
    },
    CliSpec {
        scheme: "passbolt",
        bin: "passbolt",
        version_args: &["--version"],
        min_version: "0.5.0",
    },
    CliSpec {
        scheme: "sops",
        bin: "sops",
        version_args: &["--version"],
        min_version: "3.8.0",
    },
    CliSpec {
        scheme: "infisical",
        bin: "infisical",
        version_args: &["--version"],
        min_version: "0.4.0",
    },
    CliSpec {
        scheme: "bw",
        bin: "bw",
        version_args: &["--version"],
        min_version: "2023.1.0",
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

pub fn all_supported_schemes() -> Vec<String> {
    let mut out = Vec::new();
    for scheme in Providers::new().registered_schemes() {
        out.push(scheme.to_string());
    }
    for spec in NETWORK_CLI_SPECS {
        out.push(spec.scheme.to_string());
    }
    out
}

pub fn is_secret_scheme(scheme: &str) -> bool {
    Providers::new().provider(scheme).is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatWarning {
    pub name: String,
    pub found: String,
    pub min: String,
    pub install_url: String,
}

fn parse_version(output: &str) -> Option<Version> {
    let re = Regex::new(r"(\d+)\.(\d+)\.(\d+)").expect("valid version regex");
    let caps = re.captures(output)?;
    Version::parse(&format!("{}.{}.{}", &caps[1], &caps[2], &caps[3])).ok()
}

async fn detect_version(
    spec: &CliSpec,
    name: &str,
    install_url: &str,
    extra_env: &HashMap<String, String>,
) -> Option<Version> {
    let mut cmd = vec![spec.bin];
    cmd.extend_from_slice(spec.version_args);
    let output = run_cli(&cmd, extra_env, name, install_url, None)
        .await
        .ok()?;
    parse_version(&String::from_utf8_lossy(&output.stdout))
        .or_else(|| parse_version(&String::from_utf8_lossy(&output.stderr)))
}

pub async fn check(schemes: &[String], extra_env: &HashMap<String, String>) -> Vec<CompatWarning> {
    let providers = Providers::new();
    let targets: Vec<(&'static CliSpec, &'static str, &'static str)> = schemes
        .iter()
        .filter_map(|scheme| {
            let spec = spec_for(scheme)?;
            let provider = providers.provider(scheme)?;
            Some((spec, provider.name(), provider.install_url()))
        })
        .collect();

    let checks = targets
        .into_iter()
        .map(|(spec, name, install_url)| async move {
            let found = detect_version(spec, name, install_url, extra_env).await?;
            let min = Version::parse(spec.min_version).expect("valid embedded min version");
            debug!("compat {name}: found {found}, min {min}");
            (found < min).then(|| CompatWarning {
                name: name.to_string(),
                found: found.to_string(),
                min: spec.min_version.to_string(),
                install_url: install_url.to_string(),
            })
        });
    let mut warnings = futures::future::join_all(checks)
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    warnings.extend(check_network(schemes, extra_env));
    warnings
}

fn parse_network_version(output: &str) -> Option<Version> {
    let re = Regex::new(r"(\d+)\.(\d+)(?:\.(\d+))?").expect("valid version regex");
    let captures = re.captures(output)?;
    let major = &captures[1];
    let minor = &captures[2];
    let patch = captures.get(3).map(|m| m.as_str()).unwrap_or("0");
    Version::parse(&format!("{major}.{minor}.{patch}")).ok()
}

fn check_network(schemes: &[String], extra_env: &HashMap<String, String>) -> Vec<CompatWarning> {
    let mut warnings = Vec::new();
    for spec in NETWORK_CLI_SPECS {
        if !schemes.iter().any(|scheme| scheme == spec.scheme) {
            continue;
        }
        let mut command = std::process::Command::new(spec.bin);
        command.args(spec.version_args);
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let output = command.output();
        let Ok(output) = output else {
            continue;
        };
        let found = parse_network_version(&String::from_utf8_lossy(&output.stdout))
            .or_else(|| parse_network_version(&String::from_utf8_lossy(&output.stderr)));
        let Some(found) = found else {
            continue;
        };
        let Ok(min) = Version::parse(spec.min_version) else {
            continue;
        };
        if found < min {
            warnings.push(CompatWarning {
                name: spec.name.to_string(),
                found: found.to_string(),
                min: spec.min_version.to_string(),
                install_url: spec.install_url.to_string(),
            });
        }
    }
    warnings
}

#[cfg(test)]
mod tests;
