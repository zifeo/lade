use anyhow::{Ok, Result};
use log::debug;
use std::{env, process::Command as ProcessCommand};

mod args;
mod config;
mod files;
mod global_config;
mod shell;
mod upgrade;

use args::{Args, Command, EvalCommand};
use clap::{CommandFactory, Parser};
use config::LadeFile;
use files::{hydration_or_exit, remove_files, split_env_files, write_files};
use global_config::GlobalConfig;
use shell::Shell;

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
        Command::On | Command::Off | Command::Install | Command::Uninstall => {}
        _ => upgrade::check_warn(),
    }

    let shell = Shell::detect()?;

    match command {
        Command::On => {
            println!("{}\n{}", shell.off()?, shell.on()?);
            return Ok(());
        }
        Command::Off => {
            println!("{}", shell.off()?);
            return Ok(());
        }
        Command::Install => {
            println!("Auto launcher installed in {}", shell.install()?);
            return Ok(());
        }
        Command::Uninstall => {
            println!("Auto launcher uninstalled in {}", shell.uninstall()?);
            return Ok(());
        }
        Command::Upgrade(opts) => return upgrade::perform(opts).await,
        Command::User { username, reset } => {
            let mut local_config = GlobalConfig::load().await?;
            if reset {
                local_config.user = None;
                local_config.save().await?;
                println!("Successfully reset lade user");
                return Ok(());
            }
            if let Some(user) = username {
                if user.is_empty() {
                    println!("No user provided");
                    return Ok(());
                }
                local_config.user = Some(user.clone());
                local_config.save().await?;
                println!("Successfully set user to {}", user);
                return Ok(());
            }
            if let Some(user) = local_config.user {
                println!("{}", user);
            } else {
                println!("No user set. Lade will use the current OS user.");
            }
            return Ok(());
        }
        _ => {}
    }

    let current_dir = env::current_dir()?;
    let config = LadeFile::build(current_dir.clone())?;

    match command {
        Command::Inject(EvalCommand { commands }) => {
            debug!("injecting: {:?}", commands);
            let command = commands.join(" ");

            let mut hydration = hydration_or_exit(&config, &command).await;
            let (env, files) = split_env_files(&mut hydration);
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
            let (env, files) = split_env_files(&mut hydration);
            let mut names = write_files(&files)?;
            names.extend(env.keys().cloned());
            if !names.is_empty() {
                eprintln!("Lade loaded: {}.", names.join(", "));
            }

            println!("{}", shell.set(env));
            Ok(())
        }
        Command::Unset(EvalCommand { commands }) => {
            let command = commands.join(" ");
            let mut keys = config.collect_keys(&command);
            let (env, files) = split_env_files(&mut keys);
            remove_files(&mut files.keys())?;
            println!("{}", shell.unset(env));
            Ok(())
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn verify_cli() {
        use crate::Args;
        use clap::CommandFactory;
        Args::command().debug_assert()
    }

    #[test]
    fn end_to_end() {
        use assert_cmd::Command;
        #[allow(deprecated)]
        Command::cargo_bin("lade")
            .unwrap()
            .arg("-h")
            .assert()
            .success();
    }
}
