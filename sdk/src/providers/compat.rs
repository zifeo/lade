use std::collections::HashMap;

use log::debug;
use regex::Regex;
use semver::Version;

use super::{Providers, run_cli};

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
        scheme: "vault",
        bin: "vault",
        version_args: &["version"],
        min_version: "1.11.0",
    },
    CliSpec {
        scheme: "infisical",
        bin: "infisical",
        version_args: &["--version"],
        min_version: "0.4.0",
    },
    CliSpec {
        scheme: "passbolt",
        bin: "passbolt",
        version_args: &["--version"],
        min_version: "0.5.0",
    },
];

pub fn spec_for(scheme: &str) -> Option<&'static CliSpec> {
    CLI_SPECS.iter().find(|s| s.scheme == scheme)
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
    futures::future::join_all(checks)
        .await
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::fake_cli;
    use tempfile::tempdir;

    fn path_env(dir: &tempfile::TempDir) -> HashMap<String, String> {
        HashMap::from([(
            "PATH".to_string(),
            dir.path().to_string_lossy().into_owned(),
        )])
    }

    #[test]
    fn test_parse_version_plain() {
        assert_eq!(parse_version("2.30.0"), Some(Version::new(2, 30, 0)));
    }

    #[test]
    fn test_parse_version_with_prefix() {
        assert_eq!(parse_version("v3.76.0"), Some(Version::new(3, 76, 0)));
    }

    #[test]
    fn test_parse_version_vault_format() {
        assert_eq!(
            parse_version("Vault v1.15.0 ('abc')"),
            Some(Version::new(1, 15, 0))
        );
    }

    #[test]
    fn test_parse_version_none() {
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn test_specs_have_valid_min_versions() {
        for spec in CLI_SPECS {
            assert!(
                Version::parse(spec.min_version).is_ok(),
                "{} has invalid min_version {}",
                spec.scheme,
                spec.min_version
            );
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_check_warns_on_old_version() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "op", "echo '2.0.0'");
        let warnings = check(&["op".to_string()], &path_env(&fake_bin)).await;
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].name, "1Password");
        assert_eq!(warnings[0].found, "2.0.0");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_check_ok_on_recent_version() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "op", "echo '2.30.0'");
        let warnings = check(&["op".to_string()], &path_env(&fake_bin)).await;
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_check_skips_missing_binary() {
        let empty_bin = tempdir().unwrap();
        let warnings = check(&["op".to_string()], &path_env(&empty_bin)).await;
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_check_ignores_unknown_scheme() {
        let warnings = check(&["unknown".to_string()], &HashMap::new()).await;
        assert!(warnings.is_empty());
    }
}
