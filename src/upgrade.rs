use anyhow::{Ok, Result};
use chrono::{DateTime, TimeDelta, Utc};
use self_update::{backends::github::Update, cargo_crate_version, update::UpdateStatus};
use semver::Version;
use serde::Deserialize;
use std::time::Duration;

use crate::args::UpgradeCommand;
use crate::global_config::GlobalConfig;
use crate::message_box::MessageBox;

const CHECK_INTERVAL: TimeDelta = match TimeDelta::try_days(1) {
    Some(delta) => delta,
    None => panic!("1 day is a valid TimeDelta"),
};
const FETCH_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionStatus {
    pub current: String,
    pub latest: Option<String>,
    pub update_available: bool,
}

pub fn check_is_due(update_check: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    match update_check {
        None => true,
        Some(checked_at) => checked_at + CHECK_INTERVAL < now,
    }
}

fn update_available(latest: &Option<String>, current: &str) -> bool {
    let Some(latest) = latest else {
        return false;
    };
    match (Version::parse(latest), Version::parse(current)) {
        (std::result::Result::Ok(latest), std::result::Result::Ok(current)) => latest > current,
        _ => false,
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

fn fetch_latest_tag() -> Result<String> {
    // Not `self_update::Update::get_latest_release()`: that client is built with
    // `ClientBuilder::new()` and no timeout. `tokio::time::timeout` around
    // `spawn_blocking` does not cancel the HTTP call, and dropping the runtime
    // waits for it, so `lade set` would still freeze the shell until GitHub
    // answers. `lade upgrade` keeps using self_update for the download.
    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(format!("lade/{}", cargo_crate_version!()))
        .build()?;
    let release: GithubRelease = client
        .get("https://api.github.com/repos/zifeo/lade/releases/latest")
        .send()?
        .error_for_status()?
        .json()?;
    Ok(release.tag_name.trim_start_matches('v').to_string())
}

pub async fn fetch_version_status() -> Result<VersionStatus> {
    let current = cargo_crate_version!().to_string();
    let local_config = GlobalConfig::load().await?;

    if !check_is_due(local_config.update_check, Utc::now()) {
        return Ok(VersionStatus {
            current,
            latest: None,
            update_available: false,
        });
    }

    // Stamp first so a slow or failing GitHub call is not retried on every
    // subsequent `lade set` / `lade inject` in this 24h window.
    GlobalConfig::update(|c| c.update_check = Some(Utc::now())).await?;

    let latest = tokio::task::spawn_blocking(fetch_latest_tag).await??;
    let available = update_available(&Some(latest.clone()), &current);

    Ok(VersionStatus {
        current,
        latest: Some(latest),
        update_available: available,
    })
}

pub async fn check_message() -> Result<Option<String>> {
    let status = fetch_version_status().await?;
    if status.update_available {
        return Ok(Some(format!(
            "New lade update available: {} → {}",
            status.current,
            status.latest.unwrap_or_default()
        )));
    }
    Ok(None)
}

pub async fn perform(opts: UpgradeCommand) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut update = Update::configure();
        update
            .repo_owner("zifeo")
            .repo_name("lade")
            .bin_name("lade")
            .show_download_progress(true)
            .current_version(cargo_crate_version!())
            .no_confirm(opts.yes);

        if let Some(version) = opts.version {
            update.target_version_tag(&format!("v{version}"));
        }

        match update.build()?.update_extended()? {
            UpdateStatus::UpToDate => {
                MessageBox::new()
                    .info()
                    .line("Already up to date.")
                    .print_plain_stderr();
            }
            UpdateStatus::Updated(release) => {
                MessageBox::new()
                    .info()
                    .line(format!("Updated successfully to {}.", release.version))
                    .line("")
                    .line(format!(
                        "Release notes: https://github.com/zifeo/lade/releases/tag/{}",
                        release.name
                    ))
                    .print_plain_stderr();
            }
        };
        Ok(())
    })
    .await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_checked_is_always_due() {
        assert!(check_is_due(None, Utc::now()));
    }

    #[test]
    fn recent_check_is_not_due() {
        assert!(!check_is_due(Some(Utc::now()), Utc::now()));
        assert!(!check_is_due(
            Some(Utc::now() - TimeDelta::try_hours(23).unwrap()),
            Utc::now()
        ));
    }

    #[test]
    fn day_old_check_is_due() {
        assert!(check_is_due(
            Some(Utc::now() - TimeDelta::try_hours(25).unwrap()),
            Utc::now()
        ));
    }

    #[test]
    fn newer_semver_is_an_update() {
        assert!(update_available(&Some("0.18.0".to_string()), "0.17.1"));
        assert!(!update_available(&Some("0.17.1".to_string()), "0.17.1"));
        assert!(!update_available(&None, "0.17.1"));
    }

    #[test]
    fn fetch_skips_network_when_not_due() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"update_check":"2099-01-01T00:00:00Z","user":null,"cli_check":{}}"#,
        )
        .unwrap();
        temp_env::with_vars([("LADE_CONFIG_PATH", Some(path.to_str().unwrap()))], || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let status = fetch_version_status().await.unwrap();
                    assert_eq!(status.latest, None);
                    assert!(!status.update_available);
                });
        });
    }
}
