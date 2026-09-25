use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use self_update::{backends::github::Update, cargo_crate_version, update::ReleaseStatus};
use semver::Version;
use serde::Deserialize;
use std::time::Duration;

use crate::args::UpgradeCommand;
use crate::global_config::GlobalConfig;
use crate::message_box::{MessageBox, Report};

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
            "New Lade release: {} → {}.",
            status.current,
            status.latest.unwrap_or_default()
        )));
    }
    Ok(None)
}

pub async fn nudge_if_available() {
    let Ok(Ok(status)) = tokio::time::timeout(Duration::from_secs(2), fetch_version_status()).await
    else {
        return;
    };
    if !status.update_available {
        return;
    }
    let latest = status.latest.unwrap_or_default();
    MessageBox::new()
        .info()
        .line(format!("New Lade release: {} → {latest}.", status.current))
        .line("Run `lade upgrade` to install it.")
        .print_stderr();
}

fn configure_github(bin_name: &str, yes: bool, version: Option<&str>) -> Result<Update> {
    let mut update = Update::configure();
    update
        .repo_owner("zifeo")
        .repo_name("lade")
        .bin_name(bin_name)
        .show_download_progress(true)
        .current_version(cargo_crate_version!())
        .no_confirm(yes);
    if let Some(version) = version {
        update.release_tag(format!("v{version}"));
    }
    Ok(update.build()?)
}

const PLUGIN_BIN: &str = "age-plugin-lade";

fn plugin_beside(lade: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(lade.parent()?.join(PLUGIN_BIN))
}

fn plugin_missing(lade: &std::path::Path) -> bool {
    plugin_beside(lade).is_none_or(|path| !path.is_file())
}

fn should_install_plugin(lade_updated: bool, lade_exe: Option<&std::path::Path>) -> bool {
    lade_updated || lade_exe.is_none_or(plugin_missing)
}

fn install_plugin_binary(yes: bool, version: Option<&str>) -> Result<()> {
    match configure_github(PLUGIN_BIN, yes, version)?.update() {
        std::result::Result::Ok(_) => Ok(()),
        Err(err) => {
            MessageBox::new()
                .warning()
                .paragraph(format!("{PLUGIN_BIN} was not installed: {err}."))
                .print_stderr();
            Ok(())
        }
    }
}

pub async fn perform(opts: UpgradeCommand) -> Result<()> {
    let yes = opts.yes;
    let version = opts.version.clone();
    if version.is_none() {
        match tokio::time::timeout(Duration::from_secs(2), fetch_version_status()).await {
            Ok(Ok(status)) if status.update_available => {
                let latest = status.latest.unwrap_or_default();
                MessageBox::new()
                    .info()
                    .line(format!("New Lade release: {} → {latest}.", status.current))
                    .print_stderr();
            }
            _ => {}
        }
    }
    let updated = tokio::task::spawn_blocking(move || {
        let updated = match configure_github("lade", yes, version.as_deref())?.update_extended()? {
            ReleaseStatus::UpToDate => {
                Report::new().line("Already up to date.").print();
                false
            }
            ReleaseStatus::Updated(release) => {
                Report::new()
                    .heading(format!("Updated to {}.", release.version()))
                    .line(format!(
                        "Release notes: https://github.com/zifeo/lade/releases/tag/{}",
                        release.name()
                    ))
                    .print();
                true
            }
            _ => {
                Report::new().line("Already up to date.").print();
                false
            }
        };
        let exe = std::env::current_exe().ok();
        if should_install_plugin(updated, exe.as_deref()) {
            install_plugin_binary(yes, version.as_deref())?;
        }
        Ok::<bool, anyhow::Error>(updated)
    })
    .await??;
    if updated {
        let _ = GlobalConfig::update(void_daily_stamps).await;
        if crate::mise::managed_mise_in_play() {
            crate::mise::ensure_for_setup().await.inspect_err(|e| {
                e.emit();
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
