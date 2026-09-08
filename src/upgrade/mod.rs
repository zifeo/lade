use anyhow::{Ok, Result};
use chrono::{DateTime, TimeDelta, Utc};
use self_update::{backends::github::Update, cargo_crate_version, update::ReleaseStatus};
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
    pub last_check: Option<DateTime<Utc>>,
}

pub fn check_is_due(update_check: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    match update_check {
        None => true,
        Some(checked_at) => checked_at + CHECK_INTERVAL < now,
    }
}

pub(crate) async fn stamp_daily_check(now: DateTime<Utc>, current: String) -> Result<()> {
    GlobalConfig::update(|c| {
        c.update_check = Some(now);
        c.self_version = Some(current);
    })
    .await?;
    crate::pretool::install::refresh_installed();
    Ok(())
}

pub(crate) fn void_daily_stamps(config: &mut GlobalConfig) {
    config.update_check = None;
    config.self_version = None;
}

pub fn daily_work_is_due(
    update_check: Option<DateTime<Utc>>,
    self_version: Option<&str>,
    current: &str,
    now: DateTime<Utc>,
) -> bool {
    check_is_due(update_check, now) || self_version != Some(current)
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

    if !daily_work_is_due(
        local_config.update_check,
        local_config.self_version.as_deref(),
        &current,
        Utc::now(),
    ) {
        let latest = local_config.latest_version;
        let available = update_available(&latest, &current);
        return Ok(VersionStatus {
            current,
            latest,
            update_available: available,
            last_check: local_config.update_check,
        });
    }

    // Stamp first so a slow or failing GitHub call is not retried on every
    // subsequent `lade set` / `lade inject` in this 24h window.
    let now = Utc::now();
    stamp_daily_check(now, current.clone()).await?;

    let latest = tokio::task::spawn_blocking(fetch_latest_tag).await??;
    GlobalConfig::update(|c| c.latest_version = Some(latest.clone())).await?;
    let available = update_available(&Some(latest.clone()), &current);

    Ok(VersionStatus {
        current,
        latest: Some(latest),
        update_available: available,
        last_check: Some(now),
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
    let updated = tokio::task::spawn_blocking(move || {
        let mut update = Update::configure();
        update
            .repo_owner("zifeo")
            .repo_name("lade")
            // One name. The tarball also has `age-plugin-lade` (Cargo.toml).
            .bin_name("lade")
            .show_download_progress(true)
            .current_version(cargo_crate_version!())
            .no_confirm(opts.yes);

        if let Some(version) = opts.version {
            update.release_tag(format!("v{version}"));
        }

        let updated = match update.build()?.update_extended()? {
            ReleaseStatus::UpToDate => {
                MessageBox::new()
                    .info()
                    .line("Already up to date.")
                    .print_plain_stderr();
                false
            }
            ReleaseStatus::Updated(release) => {
                MessageBox::new()
                    .info()
                    .line(format!("Updated to {}.", release.version()))
                    .line("")
                    .line(format!(
                        "Release notes: https://github.com/zifeo/lade/releases/tag/{}",
                        release.name()
                    ))
                    .print_plain_stderr();
                true
            }
            _ => {
                MessageBox::new()
                    .info()
                    .line("Already up to date.")
                    .print_plain_stderr();
                false
            }
        };
        Ok(updated)
    })
    .await??;
    if updated {
        let _ = GlobalConfig::update(void_daily_stamps).await;
        // self_update's default path is extract_file(bin_name). It does not
        // install the second Cargo binary from the tarball. Copy the new lade
        // onto age-plugin-lade so the C2SP name stays in sync.
        if let std::result::Result::Ok(exe) = std::env::current_exe()
            && let Err(err) = crate::age_plugin::copy_beside(&exe)
        {
            log::debug!("age-plugin-lade copy: {err}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
