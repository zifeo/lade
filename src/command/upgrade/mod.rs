use anyhow::Result;
use chrono::{DateTime, Utc};
use self_update::{backends::github::Update, cargo_crate_version, update::ReleaseStatus};
use semver::Version;
use serde::Deserialize;
use std::time::Duration;

use crate::args::UpgradeCommand;
use crate::hub::VersionLedger;
use crate::message_box::{MessageBox, Report};

/// What this binary's hub learned from GitHub. Nothing goes to disk, so two
/// installed Lade versions keep separate hubs and never race on a stamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionStatus {
    pub current: String,
    pub latest: Option<String>,
    pub update_available: bool,
    pub last_check: Option<DateTime<Utc>>,
    pub check_error: Option<String>,
}

impl VersionStatus {
    pub(crate) fn from_ledger(current: &str, ledger: Option<VersionLedger>) -> Self {
        let ledger = ledger.unwrap_or_default();
        Self {
            current: current.to_string(),
            update_available: update_available(&ledger.latest, current),
            latest: ledger.latest,
            last_check: ledger.tried_at_ms.and_then(DateTime::from_timestamp_millis),
            check_error: ledger.error,
        }
    }

    pub(crate) fn release_line(&self) -> Option<String> {
        let latest = self.latest.as_deref().filter(|_| self.update_available)?;
        Some(format!("New Lade release: {} → {latest}.", self.current))
    }
}

/// Asks this binary's hub. Never starts one and never hits the network.
pub fn version_status() -> VersionStatus {
    VersionStatus::from_ledger(cargo_crate_version!(), crate::hub::version_ledger())
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

/// Blocking. The hub runs it on its blocking pool.
pub(crate) fn fetch_latest_tag(timeout: Duration) -> Result<String> {
    // Not `self_update::Update::get_latest_release()`: that client is built with
    // `ClientBuilder::new()` and no timeout, and a hub stuck on GitHub would
    // never exit. `lade upgrade` keeps using self_update for the download.
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent(format!("lade/{}", cargo_crate_version!()))
        .build()?;
    let release: GithubRelease = client
        .get("https://api.github.com/repos/zifeo/lade/releases/latest")
        .send()?
        .error_for_status()?
        .json()?;
    Ok(release.tag_name.trim_start_matches('v').to_string())
}

pub fn nudge_if_available() {
    let Some(line) = version_status().release_line() else {
        return;
    };
    MessageBox::new()
        .info()
        .line(line)
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
    if version.is_none()
        && let Some(line) = version_status().release_line()
    {
        MessageBox::new().info().line(line).print_stderr();
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
        if crate::mise::managed_mise_in_play() {
            crate::mise::ensure_for_setup().await.inspect_err(|e| {
                e.emit();
            })?;
        }
        print_reload_after_upgrade();
    }
    Ok(())
}

fn print_reload_after_upgrade() {
    let shell = crate::preexec::Shell::detect().ok();
    let wrapped = shell
        .as_ref()
        .is_some_and(|s| crate::preexec::preexec_installed(s).1);
    let bin = crate::pretool::invoked_lade_bin();
    let mut message = MessageBox::new().info();
    for line in crate::preexec::reload_after_upgrade_lines(shell, &bin, wrapped) {
        message = message.line(line);
    }
    message.print_stderr();
}

#[cfg(test)]
mod tests;
