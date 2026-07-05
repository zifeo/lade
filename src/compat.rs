use std::collections::HashMap;

use anyhow::Result;
use chrono::{TimeDelta, Utc};
use log::debug;
use regex::Regex;
use rustc_hash::FxHashSet;
use semver::Version;

use lade_sdk::compat::{self, CompatWarning, spec_for};

use crate::context::InvocationContext;
use crate::global_config::GlobalConfig;
use crate::message_box::MessageBox;
use crate::provider_registry::{NETWORK_CLI_SPECS, all_supported_schemes as all_provider_schemes};

pub fn known_schemes<'a>(uris: impl Iterator<Item = &'a str>) -> Vec<String> {
    uris.filter_map(|uri| uri.split_once("://").map(|(scheme, _)| scheme))
        .filter(|scheme| {
            spec_for(scheme).is_some()
                || NETWORK_CLI_SPECS
                    .iter()
                    .any(|network| network.scheme == *scheme)
        })
        .map(|scheme| scheme.to_string())
        .collect::<FxHashSet<_>>()
        .into_iter()
        .collect()
}

pub fn all_supported_schemes() -> Vec<String> {
    all_provider_schemes()
}

/// Returns (warnings, due_schemes). Timer is only advanced when there are no warnings.
async fn check_message(schemes: Vec<String>) -> Result<(Vec<CompatWarning>, Vec<String>)> {
    if schemes.is_empty() {
        return Ok((vec![], vec![]));
    }

    let config = GlobalConfig::load().await?;
    let now = Utc::now();
    let day = TimeDelta::try_days(1).unwrap();

    let due: Vec<String> = schemes
        .into_iter()
        .filter(|scheme| match config.cli_check.get(scheme) {
            Some(last) => *last + day < now,
            None => true,
        })
        .collect();

    if due.is_empty() {
        return Ok((vec![], vec![]));
    }

    let mut warnings = compat::check(&due, &HashMap::new()).await;
    warnings.extend(check_network_compat(&due));

    if warnings.is_empty() {
        GlobalConfig::update(|c| {
            for scheme in &due {
                c.cli_check.insert(scheme.clone(), now);
            }
        })
        .await?;
    }

    Ok((warnings, due))
}

pub async fn warn_outdated(_ctx: &InvocationContext, schemes: Vec<String>) {
    match check_message(schemes).await {
        Ok((warnings, due)) if !warnings.is_empty() => {
            render(&warnings);
            let now = Utc::now();
            GlobalConfig::update(|c| {
                for scheme in &due {
                    c.cli_check.insert(scheme.clone(), now);
                }
            })
            .await
            .ok();
        }
        Ok(_) => {}
        Err(e) => debug!("CLI compatibility check failed: {e}"),
    }
}

pub async fn check_schemes(schemes: Vec<String>) -> Result<Vec<CompatWarning>> {
    Ok(check_message(schemes).await?.0)
}

fn render(warnings: &[CompatWarning]) {
    let mut box_ = MessageBox::new()
        .warning()
        .line("Some provider CLIs are older than the version Lade is tested against:")
        .line("");
    for w in warnings {
        box_ = box_.paragraph(format!(
            "{} {} is below the supported {}. Update it if you hit issues: {}",
            w.name, w.found, w.min, w.install_url
        ));
    }
    box_.line("")
        .line("Run `lade status` for details.")
        .print_stderr();
}

fn parse_version(output: &str) -> Option<Version> {
    let re = Regex::new(r"(\d+)\.(\d+)(?:\.(\d+))?").expect("valid version regex");
    let captures = re.captures(output)?;
    let major = &captures[1];
    let minor = &captures[2];
    let patch = captures.get(3).map(|m| m.as_str()).unwrap_or("0");
    Version::parse(&format!("{major}.{minor}.{patch}")).ok()
}

fn check_network_compat(schemes: &[String]) -> Vec<CompatWarning> {
    let mut warnings = Vec::new();
    for spec in NETWORK_CLI_SPECS {
        if !schemes.iter().any(|scheme| scheme == spec.scheme) {
            continue;
        }
        let output = std::process::Command::new(spec.bin)
            .args(spec.version_args)
            .output();
        let Ok(output) = output else {
            continue;
        };
        let found = parse_version(&String::from_utf8_lossy(&output.stdout))
            .or_else(|| parse_version(&String::from_utf8_lossy(&output.stderr)));
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
mod tests {
    use super::*;

    #[test]
    fn test_known_schemes_filters_and_dedupes() {
        let uris = [
            "op://my.1password.com/v/i/f",
            "op://my.1password.com/v/i/g",
            "vault://localhost/secret/app/pass",
            "plainvalue",
            "unknown://host/path",
        ];
        let mut schemes = known_schemes(uris.iter().copied());
        schemes.sort();
        assert_eq!(schemes, vec!["op".to_string(), "vault".to_string()]);
    }

    #[test]
    fn test_known_schemes_empty() {
        assert!(known_schemes(["plain".to_string()].iter().map(|s| s.as_str())).is_empty());
    }

    #[test]
    fn test_all_supported_schemes() {
        assert!(all_supported_schemes().contains(&"op".to_string()));
    }
}
