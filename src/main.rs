use anyhow::{Ok, Result};
use log::debug;
<<<<<<< HEAD
<<<<<<< HEAD
use std::{env, io::Read};
||||||| parent of d44c633 (feat: agentic hook support (claude/cursor) (#131))
use std::{env, process::Command as ProcessCommand};
=======
use std::{env, io::Read, process::Command as ProcessCommand};
>>>>>>> d44c633 (feat: agentic hook support (claude/cursor) (#131))
||||||| parent of fb52f33 (refactor: simplify)
use std::{env, io::Read};
=======
use std::{env, io::Read, time::Duration};
>>>>>>> fb52f33 (refactor: simplify)

mod args;
mod config;
mod exec;
mod files;
mod global_config;
<<<<<<< HEAD
mod hook;
mod redact;
||||||| parent of d44c633 (feat: agentic hook support (claude/cursor) (#131))
=======
mod hook;
>>>>>>> d44c633 (feat: agentic hook support (claude/cursor) (#131))
mod shell;
mod upgrade;

use args::{Args, Command, EvalCommand};
use clap::{CommandFactory, Parser};
use config::LadeFile;
use files::{hydration_or_exit, remove_files, split_env_files, write_files};
use global_config::GlobalConfig;
use redact::Redactor;
use lade_sdk::hydrate_one;
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

<<<<<<< HEAD
    match command {
        Command::On
        | Command::Off
        | Command::Install
        | Command::Uninstall
        | Command::Eval { .. } => {}
        _ => upgrade::check_warn(),
    }
||||||| parent of 6173758 (refactor: simplify)
    match command {
        Command::On | Command::Off | Command::Install | Command::Uninstall => {}
        _ => upgrade::check_warn(),
    }
=======
    let upgrade_task = match command {
        Command::On | Command::Off | Command::Install | Command::Uninstall => None,
        _ => Some(tokio::spawn(upgrade::check_message())),
    };
>>>>>>> 6173758 (refactor: simplify)

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
            let width = 80;
            let wrap_width = width - 4;
            let header = "Lade could not parse a config file:";
            let hint = "Hint: check the file format.";
            let error = e.to_string();
            eprintln!("┌{}┐", "-".repeat(width - 2));
            eprintln!("| {} {}|", header, " ".repeat(wrap_width - header.len()));
            for line in textwrap::wrap(error.trim(), wrap_width - 2) {
                eprintln!(
                    "| > {} {}|",
                    line,
                    " ".repeat(wrap_width - 2 - textwrap::core::display_width(&line)),
                );
            }
            eprintln!("| {} {}|", hint, " ".repeat(wrap_width - hint.len()));
            eprintln!("└{}┘", "-".repeat(width - 2));
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

            let mut hydration = hydration_or_exit(&config, &command).await;
            let (env, files) = split_env_files(&mut hydration);
            let mut names = write_files(&files)?;
            names.extend(env.keys().cloned());
            if !names.is_empty() {
                eprintln!("Lade loaded: {}.", names.join(", "));
            }

            let redactor = if !opts.no_mask {
                let mut all_secrets = env.clone();
                for vars in files.values() {
                    all_secrets.extend(vars.clone());
                }
                Redactor::new(&all_secrets, &opts.mask_format)
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

            let mut hydration = hydration_or_exit(&config, &command).await;
            let (env, files) = split_env_files(&mut hydration);
            let mut names = write_files(&files)?;
            names.extend(env.keys().cloned());
            if !names.is_empty() {
                eprintln!("Lade loaded: {}.", names.join(", "));
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
        && let anyhow::Result::Ok(anyhow::Result::Ok(anyhow::Result::Ok(Some(msg)))) =
            tokio::time::timeout(Duration::from_millis(50), task).await
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
