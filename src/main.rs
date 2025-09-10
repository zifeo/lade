use anyhow::{Ok, Result, bail};
use chrono::{TimeDelta, Utc};
use log::{debug, warn};
use self_update::{backends::github::Update, cargo_crate_version, update::UpdateStatus};
use semver::Version;
use std::{
    collections::{HashMap, hash_map::Keys},
    env,
    ffi::OsStr,
    fs,
    path::PathBuf,
    process::Command as ProcessCommand,
};
use tokio::time;
mod config;
mod shell;
use clap::{CommandFactory, Parser};
use shell::Shell;
mod args;
mod global_config;
use args::{Args, Command, EvalCommand};
use global_config::GlobalConfig;

use config::{Config, LadeFile, Output};

async fn upgrade_check() -> Result<()> {
    let project = directories::ProjectDirs::from("com", "zifeo", "lade")
        .expect("cannot get directory for projet");

    let config_path = project.config_local_dir().join("config.json");
    debug!("config_path: {:?}", config_path);
    let mut local_config = GlobalConfig::load().await?;

    if local_config.update_check + TimeDelta::try_days(1).unwrap() < Utc::now() {
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
        local_config.save().await?;
    }
    Ok(())
}

async fn hydration_or_exit(
    config: &Config,
    command: &str,
) -> HashMap<Output, HashMap<String, String>> {
    match config.collect_hydrate(command).await {
        std::result::Result::Ok(hydration) => hydration,
        Err(e) => {
            let width = 80;
            let wrap_width = width - 4;

            let header = "Lade could not get secrets from one loader:";
            let error = e.to_string();
            let hint = "Hint: check whether the loader is connected? to the correct? vault.";
            let wait = "Waiting 5 seconds before continuing...";

            eprintln!("┌{}┐", "-".repeat(width - 2));
            eprintln!("| {} {}|", header, " ".repeat(wrap_width - header.len()),);
            for line in textwrap::wrap(error.trim(), wrap_width - 2) {
                eprintln!(
                    "| > {} {}|",
                    line,
                    " ".repeat(wrap_width - 2 - textwrap::core::display_width(&line)),
                );
            }
            eprintln!("| {} {}|", hint, " ".repeat(wrap_width - hint.len()));
            eprintln!("| {} {}|", wait, " ".repeat(wrap_width - wait.len()));
            eprintln!("└{}┘", "-".repeat(width - 2));
            time::sleep(time::Duration::from_secs(5)).await;
            std::process::exit(1);
        }
    }
}

fn write_files(hydration: &HashMap<PathBuf, HashMap<String, String>>) -> Result<Vec<String>> {
    let mut names = vec![];

    for (path, vars) in hydration {
        names.extend(vars.keys().cloned());

        if path.exists() {
            bail!("file already exists: {:?}", path)
        }
        debug!("writing file: {:?}", path);
        let content: String = match path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or_else(|| panic!("cannot get extension of file: {:?}", path.display()))
        {
            "json" => serde_json::to_string(&vars)?,
            "yaml" | "yml" => serde_yaml::to_string(&vars)?,
            _ => bail!("unsupported file extension: {:?}", path.extension()),
        };
        fs::write(path, content)?;
    }
    Ok(names)
}

fn remove_files<T>(files: &mut Keys<PathBuf, T>) -> Result<()> {
    for path in files {
        debug!("removing file: {:?}", path);
        if !path.exists() {
            bail!("file should have existed: {:?}", path)
        }
        fs::remove_file(path)?;
    }
    Ok(())
}

fn split_env_files<T: Default + ToOwned>(
    hydration: &mut HashMap<Output, T>,
) -> Result<(T, HashMap<PathBuf, T>)>
where
    HashMap<PathBuf, T>: FromIterator<(PathBuf, <T as ToOwned>::Owned)>,
{
    let env = hydration
        .remove(&None::<PathBuf>)
        .unwrap_or_else(|| Default::default());
    let files = hydration
        .iter_mut()
        .filter(|(path, _)| path.is_some())
        .map(|(path, vars)| {
            (
                path.to_owned().unwrap(), // cannot panic as None has been removed above
                vars.to_owned(),
            )
        })
        .collect();
    Ok((env, files))
}

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_family = "unix")]
    {
        // fix the pipe: https://github.com/rust-lang/rust/issues/46016
        use nix::sys::signal;
        unsafe {
            signal::signal(signal::Signal::SIGPIPE, signal::SigHandler::SigDfl)?;
        }
    }

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
    let config = LadeFile::build(current_dir.clone())?;
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
        Command::Inject(EvalCommand { commands }) => {
            debug!("injecting: {:?}", commands);
            let command = commands.join(" ");

            let mut hydration = hydration_or_exit(&config, &command).await;
            let (env, files) = split_env_files(&mut hydration)?;

            let mut names = write_files(&files)?;

            names.extend(env.keys().cloned());
            if !names.is_empty() {
                eprintln!("Lade loaded: {}.", names.join(", "));
            }

            let status = ProcessCommand::new(shell.bin())
                .args(["-c", &command])
                .current_dir(current_dir)
                .envs(env::vars())
                .envs(env)
                .status();

            remove_files(&mut files.keys())?;

            let status = status.expect("failed to execute command");
            if !status.success() {
                eprintln!("command failed");
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Set(EvalCommand { commands }) => {
            debug!("setting: {:?}", commands);
            let command = commands.join(" ");

            let mut hydration = hydration_or_exit(&config, &command).await;
            let (env, files) = split_env_files(&mut hydration)?;

            let mut names = write_files(&files)?;

            names.extend(env.keys().cloned());
            if !names.is_empty() {
                eprintln!("Lade loaded: {}.", names.join(", "));
            }

            println!("{}", shell.set(env));
            Ok(())
        }
        Command::Unset(EvalCommand { commands }) => {
            debug!("unsetting: {:?}", commands);
            let command = commands.join(" ");

            let mut keys = config.collect_keys(&command);
            let (env, files) = split_env_files(&mut keys)?;

            remove_files(&mut files.keys())?;
            println!("{}", shell.unset(env));

            Ok(())
        }
        Command::On => {
            println!("{}\n{}", shell.off()?, shell.on()?);
            Ok(())
        }
        Command::Off => {
            println!("{}", shell.off()?);
            Ok(())
        }
        Command::Install => {
            println!("Auto launcher installed in {}", shell.install()?);
            Ok(())
        }
        Command::Uninstall => {
            println!("Auto launcher uninstalled in {}", shell.uninstall()?);
            Ok(())
        }
        Command::SetUser { user } => {
            let mut local_config = GlobalConfig::load().await?;

            if user.is_empty() {
                println!("no user provided");
                return Ok(());
            }

            local_config.user = Some(user);
            let _ = local_config.save().await?;

            Ok(())
        }
        Command::GetUser => {
            let local_config = GlobalConfig::load().await?;
            println!(
                "{}",
                local_config
                    .user
                    .unwrap_or("no user set. please use lade set-user to set a user".to_string())
            );
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
