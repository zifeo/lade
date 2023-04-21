use anyhow::{Ok, Result};
use chrono::{DateTime, Duration, Utc};
use clap::Subcommand;
use clap_verbosity_flag::Verbosity;
use log::{debug, info};
use self_update::{backends::github::Update, cargo_crate_version, update::UpdateStatus};
use serde::{Deserialize, Serialize};
use std::{env, path::Path};
mod config;
mod shell;
use clap::Parser;
use shell::Shell;
use tokio::fs;

use config::LadeFile;

#[derive(Parser, Debug)]
pub struct UpgradeCommand {
    /// Upgrade to specific version (e.g. 1.0.0)
    #[clap(long)]
    version: Option<String>,

    /// Do not ask for version confirmation
    #[clap(short, long, default_value_t = false)]
    yes: bool,
}

#[derive(Parser, Debug)]
pub struct EvalCommand {
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    commands: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Upgrade Lade.
    Upgrade(UpgradeCommand),
    /// Enable execution hooks.
    On,
    /// Disable execution hooks.
    Off,
    /// Set environment for shell.
    Set(EvalCommand),
    /// Unset environment for shell.
    Unset(EvalCommand),
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: Command,

    #[command(flatten)]
    verbose: Verbosity,
}

#[derive(Deserialize, Serialize)]
struct LocalConfig {
    update_check: DateTime<Utc>,
}

impl LocalConfig {
    async fn from<P: AsRef<Path>>(path: P) -> Result<Self> {
        if path.as_ref().exists() {
            let config_str = fs::read_to_string(path).await?;
            let config: LocalConfig = serde_json::from_str(&config_str)?;
            Ok(config)
        } else {
            let config = LocalConfig {
                update_check: Utc::now(),
            };
            config.save(path).await?;
            Ok(config)
        }
    }
    async fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let config_str = serde_json::to_string_pretty(&self)?;
        fs::create_dir_all(path.as_ref().parent().unwrap()).await?;
        fs::write(path, config_str).await?;
        Ok(())
    }
}

async fn upgrade_check() -> Result<()> {
    let project = directories::ProjectDirs::from("com", "zifeo", "lade")
        .expect("cannot get directory for projet");

    let config_path = project.config_local_dir().join("config.json");
    debug!("config_path: {:?}", config_path);
    let mut local_config = LocalConfig::from(config_path.clone()).await?;

    if local_config.update_check + Duration::days(1) < Utc::now() {
        debug!("checking for update");
        tokio::task::spawn_blocking(move || {
            let update = Update::configure()
                .repo_owner("zifeo")
                .repo_name("lade")
                .bin_name("lade")
                .current_version(cargo_crate_version!())
                .build()?;

            let latest = update.get_latest_release()?;
            if latest.version != update.current_version() {
                println!(
                    "New lade update available: {} -> {} (use: lade upgrade)",
                    update.current_version(),
                    latest.version
                );
            }

            Ok(())
        })
        .await??;

        local_config.update_check = Utc::now();
        local_config.save(config_path).await?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    env_logger::Builder::new()
        .filter_level(args.verbose.log_level_filter())
        .init();

    upgrade_check()
        .await
        .unwrap_or_else(|e| info!("cannot check for update: {}", e));

    let current_dir = env::current_dir()?;
    let config = LadeFile::build(current_dir)?;
    let shell = Shell::from_env()?;

    match args.command {
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
            let command = commands.join(" ");
            let vars = config.collect_hydrate(command).await?;
            println!("{}", shell.set(vars));
            Ok(())
        }
        Command::Unset(EvalCommand { commands }) => {
            let command = commands.join(" ");
            let vars = config.collect_keys(command);
            println!("{}", shell.unset(vars));
            Ok(())
        }
        Command::On => {
            println!("{};{}", shell.off(), shell.on());
            Ok(())
        }
        Command::Off => {
            println!("{}", shell.off());
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
