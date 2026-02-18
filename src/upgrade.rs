use anyhow::{Ok, Result};
use chrono::{TimeDelta, Utc};
use log::warn;
use self_update::{backends::github::Update, cargo_crate_version, update::UpdateStatus};
use semver::Version;

use crate::args::UpgradeCommand;
use crate::global_config::GlobalConfig;

pub async fn check() -> Result<()> {
    let mut local_config = GlobalConfig::load().await?;

    if local_config.update_check + TimeDelta::try_days(1).unwrap() < Utc::now() {
        let current_version = cargo_crate_version!();
        let latest = tokio::task::spawn_blocking(move || {
            let update = Update::configure()
                .repo_owner("zifeo")
                .repo_name("lade")
                .bin_name("lade")
                .current_version(current_version)
                .build()?;
            Ok(update.get_latest_release()?)
        })
        .await??;
        if Version::parse(&latest.version)? > Version::parse(current_version)? {
            eprintln!(
                "New lade update available: {} -> {} (use: lade upgrade)",
                current_version, latest.version
            );
        }
        local_config.update_check = Utc::now();
        local_config.save().await?;
    }
    Ok(())
}

pub fn check_warn() {
    tokio::spawn(async {
        check()
            .await
            .unwrap_or_else(|e| warn!("cannot check for update: {}", e));
    });
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
            UpdateStatus::UpToDate => println!("Already up to date!"),
            UpdateStatus::Updated(release) => {
                println!("Updated successfully to {}!", release.version);
                println!(
                    "Release notes: https://github.com/zifeo/lade/releases/tag/{}",
                    release.name
                );
            }
        };
        Ok(())
    })
    .await??;
    Ok(())
}
