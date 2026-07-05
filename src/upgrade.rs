use anyhow::{Ok, Result};
use chrono::{TimeDelta, Utc};
use self_update::{backends::github::Update, cargo_crate_version, update::UpdateStatus};
use semver::Version;

use crate::args::UpgradeCommand;
use crate::global_config::GlobalConfig;
use crate::message_box::MessageBox;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionStatus {
    pub current: String,
    pub latest: Option<String>,
    pub update_available: bool,
}

pub async fn fetch_version_status() -> Result<VersionStatus> {
    let current = cargo_crate_version!().to_string();
    let local_config = GlobalConfig::load().await?;
    let day = TimeDelta::try_days(1).unwrap();

    if local_config.update_check + day >= Utc::now() {
        return Ok(VersionStatus {
            current,
            latest: None,
            update_available: false,
        });
    }

    let current_for_check = current.clone();
    let latest = tokio::task::spawn_blocking(move || {
        let update = Update::configure()
            .repo_owner("zifeo")
            .repo_name("lade")
            .bin_name("lade")
            .current_version(current_for_check.as_str())
            .build()?;
        Ok(update.get_latest_release()?)
    })
    .await??;

    let update_available = Version::parse(&latest.version)? > Version::parse(&current)?;
    if !update_available {
        GlobalConfig::update(|c| c.update_check = Utc::now()).await?;
    }

    Ok(VersionStatus {
        current,
        latest: Some(latest.version),
        update_available,
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
