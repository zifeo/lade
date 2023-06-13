use anyhow::{bail, Ok, Result};
use chrono::{Duration, Utc};
use log::{debug, warn};
use self_update::{backends::github::Update, cargo_crate_version, update::UpdateStatus};
use semver::Version;
use std::{env, ffi::OsStr, fs};
mod config;
mod shell;
use clap::{CommandFactory, Parser};
use shell::Shell;
mod args;
mod global_config;
use args::{Args, Command, EvalCommand};
use global_config::GlobalConfig;

use config::LadeFile;

async fn upgrade_check() -> Result<()> {
    let project = directories::ProjectDirs::from("com", "zifeo", "lade")
        .expect("cannot get directory for projet");

    let config_path = project.config_local_dir().join("config.json");
    debug!("config_path: {:?}", config_path);
    let mut local_config = GlobalConfig::load(config_path.clone()).await?;

    if local_config.update_check + Duration::days(1) < Utc::now() {
        debug!("checking for update");
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
            println!(
                "New lade update available: {} -> {} (use: lade upgrade)",
                current_version, latest.version
            );
        }

        local_config.update_check = Utc::now();
        local_config.save(config_path).await?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::try_parse()?;

    env_logger::Builder::new()
        .filter_level(args.verbose.log_level_filter())
        .init();

    if args.version {
        println!("lade {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.help {
        Args::command().print_help()?;
        return Ok(());
    }

    let command = match args.command {
        Some(command) => command,
        None => {
            Args::command().print_help()?;
            return Ok(());
        }
    };

    match command {
        Command::On | Command::Off => {}
        _ => {
            upgrade_check()
                .await
                .unwrap_or_else(|e| warn!("cannot check for update: {}", e));
        }
    }

    let current_dir = env::current_dir()?;
    let config = LadeFile::build(current_dir)?;
    let shell = Shell::detect()?;

    match command {
        Command::Upgrade(opts) => {
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
        Command::Set(EvalCommand { commands }) => {
            debug!("setting: {:?}", commands);
            let command = commands.join(" ");
            for (output, vars) in config.collect_hydrate(command).await? {
                match output {
                    Some(path) => {
                        if path.exists() {
                            bail!("file already exists: {:?}", path)
                        }
                        debug!("writing file: {:?}", path);
                        let content: String =
                            match path.extension().and_then(OsStr::to_str).unwrap_or_else(|| {
                                panic!("cannot get extension of file: {:?}", path.display())
                            }) {
                                "json" => serde_json::to_string(&vars)?,
                                "yaml" | "yml" => serde_yaml::to_string(&vars)?,
                                _ => bail!("unsupported file extension: {:?}", path.extension()),
                            };
                        fs::write(path, content)?;
                    }
                    None => {
                        println!("{}", shell.set(vars));
                    }
                }
            }
            Ok(())
        }
        Command::Unset(EvalCommand { commands }) => {
            debug!("unsetting: {:?}", commands);
            let command = commands.join(" ");
            for (output, vars) in config.collect_keys(command) {
                match output {
                    Some(path) => {
                        debug!("removing file: {:?}", path);
                        if !path.exists() {
                            bail!("file should have existed: {:?}", path)
                        }
                        fs::remove_file(path)?;
                    }
                    None => {
                        println!("{}", shell.unset(vars));
                    }
                }
            }
            Ok(())
        }
        Command::On => {
            println!("{}\n{}", shell.off(), shell.on());
            Ok(())
        }
        Command::Off => {
            println!("{}", shell.off());
            Ok(())
        }
        Command::Install => {
            println!("Auto launcher installed in {}", shell.install());
            Ok(())
        }
        Command::Uninstall => {
            println!("Auto launcher uninstalled in {}", shell.uninstall());
            Ok(())
        }
    }
}

#[test]
fn verify_cli() {
    use crate::Args;
    use clap::CommandFactory;
    Args::command().debug_assert()
}

#[test]
fn end_to_end() {
    // need build before running this test
    use assert_cmd::Command;

    let mut cmd = Command::cargo_bin("lade").unwrap();
    cmd.arg("-h").assert().success();
}
