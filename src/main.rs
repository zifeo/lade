use anyhow::{Ok, Result};
use log::debug;
use std::{env, io::Read, time::Duration};

mod args;
mod config;
mod disclaimer;
mod exec;
mod files;
mod global_config;
mod hook;
mod masking;
mod message_box;
mod redact;
mod shell;
mod upgrade;

use args::{Args, Command, EvalCommand};
use clap::{CommandFactory, Parser};
use config::LadeFile;
use files::{LoadedSecrets, hydration_or_exit, remove_files, split_env_files, write_files};
use global_config::GlobalConfig;
use lade_sdk::hydrate_one;
use redact::Redactor;
use shell::Shell;

fn main() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<()> {
    #[cfg(target_family = "unix")]
    {
        // fix the pipe: https://github.com/rust-lang/rust/issues/46016
        use nix::sys::signal;
        unsafe {
            signal::signal(signal::Signal::SIGPIPE, signal::SigHandler::SigDfl)?;
        }
    }

    let args = Args::try_parse()?;

    let mut builder = env_logger::Builder::new();
    match env::var("LADE_LOG").ok().filter(|s| !s.is_empty()) {
        Some(filter) => {
            builder.parse_filters(&filter);
        }
        None => {
            builder.filter_level(args.verbose.log_level_filter());
        }
    };
    builder.init();

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

    let upgrade_task = match command {
        Command::On
        | Command::Off
        | Command::Install
        | Command::Uninstall
        | Command::Eval { .. } => None,
        _ => Some(tokio::spawn(upgrade::check_message())),
    };

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
            if reset {
                GlobalConfig::update(|c| c.user = None).await?;
                println!("Successfully reset lade user");
                return Ok(());
            }
            if let Some(user) = username {
                if user.is_empty() {
                    println!("No user provided");
                    return Ok(());
                }
                GlobalConfig::update(|c| c.user = Some(user.clone())).await?;
                println!("Successfully set user to {}", user);
                return Ok(());
            }
            let config = GlobalConfig::load().await?;
            if let Some(user) = config.user {
                println!("{}", user);
            } else {
                println!("No user set. Lade will use the current OS user.");
            }
            return Ok(());
        }
        _ => {}
    }

    let current_dir = env::current_dir()?;

    if let Command::Eval { uri } = command {
        let value = hydrate_one(uri, &current_dir, &std::collections::HashMap::new()).await?;
        println!("{}", value);
        return Ok(());
    }

    let config = match LadeFile::build(current_dir.clone()) {
        std::result::Result::Ok(c) => c,
        Err(e) => {
            message_box::MessageBox::new()
                .line("Lade could not parse a config file:")
                .paragraph(e.to_string())
                .line("Hint: check the file format.")
                .print_stderr();
            std::process::exit(1);
        }
    };

    let mut inject_exit_code: Option<i32> = None;

    match command {
        Command::Hook => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let output = hook::handle(&config, &input)?;
            print!("{}", output);
        }
        Command::Inject(opts) => {
            debug!("injecting: {:?}", opts.commands);
            let command = opts.commands.join(" ");

            disclaimer::prompt(&config.collect_disclaimers(&command))?;

            let LoadedSecrets {
                mut vars,
                sources,
                maskable,
            } = hydration_or_exit(&config, &command).await;
            let (env, files) = split_env_files(&mut vars);
            let mut names = write_files(&files)?;
            names.extend(env.keys().cloned());
            if !names.is_empty() {
                names.sort();
                eprintln!("Lade loaded: {}.", names.join(", "));
                eprintln!();
            }

            let redactor = if !opts.no_mask {
                Redactor::new(
                    &masking::secrets_for_redaction(&env, &files, &sources, &maskable),
                    &opts.mask_format,
                )
            } else {
                None
            };

            let code = exec::run(shell.bin(), &command, env, &current_dir, redactor);
            remove_files(&mut files.keys())?;

            let code = code?;
            if code != 0 {
                eprintln!("command failed");
                inject_exit_code = Some(code);
            }
        }
        Command::Set(EvalCommand { commands }) => {
            debug!("setting: {:?}", commands);
            let command = commands.join(" ");

            disclaimer::prompt(&config.collect_disclaimers(&command))?;

            let LoadedSecrets { mut vars, .. } = hydration_or_exit(&config, &command).await;
            let (env, files) = split_env_files(&mut vars);
            let mut names = write_files(&files)?;
            names.extend(env.keys().cloned());
            if !names.is_empty() {
                names.sort();
                eprintln!("Lade loaded: {}.", names.join(", "));
                eprintln!();
            }

            println!("{}", shell.set(env));
        }
        Command::Unset(EvalCommand { commands }) => {
            let command = commands.join(" ");
            let mut keys = config.collect_keys(&command);
            let (env, files) = split_env_files(&mut keys);
            remove_files(&mut files.keys())?;
            println!("{}", shell.unset(env));
        }
        _ => unreachable!(),
    }

    if let Some(task) = upgrade_task
        && let Some(msg) = tokio::time::timeout(Duration::from_millis(50), task)
            .await
            .ok()
            .and_then(|r| r.ok())
            .and_then(|r| r.ok())
            .flatten()
    {
        eprintln!("{msg}");
    }

    if let Some(code) = inject_exit_code {
        std::process::exit(code);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn verify_cli() {
        use crate::Args;
        use clap::CommandFactory;
        Args::command().debug_assert()
    }
}
