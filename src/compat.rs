use std::collections::HashMap;

use anyhow::Result;
use chrono::{TimeDelta, Utc};
use log::debug;
use rustc_hash::FxHashSet;

use lade_sdk::compat::{self, CompatWarning, spec_for};

use crate::global_config::GlobalConfig;
use crate::message_box::MessageBox;
use crate::prompt;

pub fn known_schemes<'a>(uris: impl Iterator<Item = &'a str>) -> Vec<String> {
    uris.filter_map(|uri| uri.split_once("://").map(|(scheme, _)| scheme))
        .filter(|scheme| spec_for(scheme).is_some())
        .map(|scheme| scheme.to_string())
        .collect::<FxHashSet<_>>()
        .into_iter()
        .collect()
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

    let warnings = compat::check(&due, &HashMap::new()).await;

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

pub async fn warn_outdated(schemes: Vec<String>) {
    match check_message(schemes).await {
        Ok((warnings, due)) if !warnings.is_empty() => {
            render(&warnings);
            if let Some(offset) = prompt::ask_snooze_offset().await {
                GlobalConfig::update(|c| {
                    for scheme in &due {
                        c.cli_check.insert(scheme.clone(), Utc::now() + offset);
                    }
                })
                .await
                .ok();
            }
        }
        Ok(_) => {}
        Err(e) => debug!("CLI compatibility check failed: {e}"),
    }
}

fn render(warnings: &[CompatWarning]) {
    let mut box_ = MessageBox::new()
        .warning()
        .line("Some secret CLIs are older than the version Lade is tested against:");
    for w in warnings {
        box_ = box_.paragraph(format!(
            "{} {} is below the supported {}. Update it if you hit issues: {}",
            w.name, w.found, w.min, w.install_url
        ));
    }
    box_.print_stderr();
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
}
