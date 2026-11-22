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
    // § d1d316x4b.d@rbm64
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
    // § d1d316x4b.b@de9ht
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

fn configure_github(
    bin_name: &str,
    yes: bool,
    version: Option<&str>,
    install_path: Option<&std::path::Path>,
) -> Result<Update> {
    // § tp05szjjy.a@3a2s7
    let mut update = Update::configure();
    update
        .repo_owner("zifeo")
        .repo_name("lade")
        .bin_name(bin_name)
        .show_download_progress(true)
        .current_version(cargo_crate_version!())
        .no_confirm(yes)
        .verify_archive(verify_release_archive);
    if let Some(version) = version {
        update.release_tag(format!("v{version}"));
    }
    if let Some(path) = install_path {
        update.bin_install_path(path);
    }
    Ok(update.build()?)
}

const RELEASE_DOWNLOAD: &str = "https://github.com/zifeo/lade/releases/download";

/// The URL of the `.sha256` the release build publishes beside `asset`.
fn checksum_url(asset: &str, target: &str) -> Result<String> {
    // § tp05szjjy.a@gk6f4
    let version = asset
        .strip_prefix("lade-v")
        .and_then(|rest| rest.strip_suffix(&format!("-{target}.tar.gz")))
        .ok_or_else(|| anyhow::anyhow!("{asset} is not a Lade release archive for {target}"))?;
    Ok(format!("{RELEASE_DOWNLOAD}/v{version}/{asset}.sha256"))
}

/// The hex digest a `sha256sum` line (`<hex>  <asset>`) or a bare digest gives.
fn expected_checksum(sums: &str, asset: &str) -> Result<String> {
    let mut words = sums.split_whitespace();
    let hex = words
        .next()
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow::anyhow!("{asset}.sha256 holds no SHA256 digest"))?;
    if let Some(name) = words.next()
        && name.trim_start_matches('*') != asset
    {
        anyhow::bail!("{asset}.sha256 names {name}, not {asset}");
    }
    Ok(hex.to_ascii_lowercase())
}

fn verify_sha256(archive: &std::path::Path, expected: &str) -> Result<()> {
    // § tp05szjjy.b@pf8mc
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(archive)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        anyhow::bail!("checksum mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

/// Blocking. `Ok(None)` when the release published no checksum.
fn fetch_checksum(url: &str) -> Result<Option<String>> {
    // § tp05szjjy.b@nsjzb
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("lade/{}", cargo_crate_version!()))
        .build()?;
    let response = client.get(url).send()?;
    // § one tp05szjjy.e@gwwg5
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    Ok(Some(response.error_for_status()?.text()?))
}

fn verify_release_archive(archive: &std::path::Path) -> self_update::errors::Result<()> {
    match check_release_archive(archive, self_update::get_target(), fetch_checksum) {
        Ok(Some(warning)) => {
            MessageBox::new()
                .warning()
                .paragraph(warning)
                .print_stderr();
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(err) => Err(self_update::errors::Error::archive_verification_rejected(
            format!("{err:#}"),
        )),
    }
}

/// `Ok(Some(warning))`: the release published no checksum, and the archive
/// installs unverified.
fn check_release_archive(
    archive: &std::path::Path,
    target: &str,
    fetch: impl FnOnce(&str) -> Result<Option<String>>,
) -> Result<Option<String>> {
    let asset = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("{} has no file name", archive.display()))?;
    let url = checksum_url(asset, target)?;
    match fetch(&url)? {
        Some(sums) => {
            let expected = expected_checksum(&sums, asset)?;
            verify_sha256(archive, &expected)?;
            Ok(None)
        }
        None => {
            // § tp05szjjy.e@xv2ht
            Ok(Some(format!(
                "No checksum published ({asset}.sha256 is missing). Installing {asset} unverified."
            )))
        }
    }
}

const PLUGIN_BIN: &str = "age-plugin-lade";

fn plugin_beside(lade: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(lade.parent()?.join(PLUGIN_BIN))
}

fn plugin_missing(lade: &std::path::Path) -> bool {
    plugin_beside(lade).is_none_or(|path| !path.is_file())
}

fn should_install_plugin(lade_updated: bool, lade_exe: &std::path::Path) -> bool {
    // § tzctg3bsv.a@fxpk6
    lade_updated || plugin_missing(lade_exe)
}

/// The plugin update writes beside `lade_exe`, from the release `lade_version` names.
fn configure_plugin(lade_exe: &std::path::Path, yes: bool, lade_version: &str) -> Result<Update> {
    // § tzctg3bsv.a@wb7as
    // self_update installs over the running executable when no path is set, and with no
    // tag it compares the plugin against Lade's own version, so a current Lade finds
    // nothing newer and a missing plugin would stay missing.
    let path = plugin_beside(lade_exe)
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", lade_exe.display()))?;
    configure_github(PLUGIN_BIN, yes, Some(lade_version), Some(&path))
}

fn install_plugin_binary(lade_exe: &std::path::Path, yes: bool, lade_version: &str) -> Result<()> {
    match configure_plugin(lade_exe, yes, lade_version)?.update() {
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
    // § tzctg3bsv@bhmjp
    let yes = opts.yes;
    let version = opts.version.clone();
    if version.is_none()
        && let Some(line) = version_status().release_line()
    {
        MessageBox::new().info().line(line).print_stderr();
    }
    let updated = tokio::task::spawn_blocking(move || {
        // Read before the update: once replaced, Linux reports the path with " (deleted)".
        let exe = std::env::current_exe()?;
        let (updated, lade_version) =
            match configure_github("lade", yes, version.as_deref(), None)?.update_extended()? {
                ReleaseStatus::Updated(release) => {
                    Report::new()
                        .heading(format!("Updated to {}.", release.version()))
                        .line(format!(
                            "Release notes: https://github.com/zifeo/lade/releases/tag/{}",
                            release.name()
                        ))
                        .print();
                    (true, release.version().to_string())
                }
                _ => {
                    Report::new().line("Already up to date.").print();
                    (false, cargo_crate_version!().to_string())
                }
            };
        if should_install_plugin(updated, &exe) {
            install_plugin_binary(&exe, yes, &lade_version)?;
        }
        Ok::<bool, anyhow::Error>(updated)
    })
    .await??;
    if updated {
        after_update(print_reload_after_upgrade, async {
            if crate::mise::managed_mise_in_play() {
                crate::mise::ensure_managed().await.inspect_err(|e| {
                    e.emit();
                })?;
            }
            Ok(())
        })
        .await?;
    }
    Ok(())
}

/// The new binary is already in place, so the reload hint holds even when
/// the mise step after it fails.
async fn after_update(
    reload: impl FnOnce(),
    ensure_mise: impl std::future::Future<Output = Result<()>>,
) -> Result<()> {
    // § tzctg3bsv.c@mz90g
    reload();
    ensure_mise.await
}

fn print_reload_after_upgrade() {
    // § tzctg3bsv.c@9ywgm
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
