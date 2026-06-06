use anyhow::{Ok, Result};
use log::debug;
use rustc_hash::FxHashSet;
use std::{collections::HashMap, env, io::IsTerminal, io::Read, path::PathBuf, time::Duration};

mod args;
mod compat;
mod config;
mod exec;
mod files;
mod global_config;
mod hook;
mod masking;
mod message_box;
mod prompt;
mod redact;
mod shell;
mod upgrade;

use args::{Args, Command, EvalCommand};
use clap::{CommandFactory, Parser};
use config::{Config, LadeFile};
use files::{
    LoadedSecrets, hydration_or_exit, remove_files, sleep_or_cancel, split_env_files, write_files,
};
use global_config::GlobalConfig;
use lade_sdk::hydrate_one;
use redact::Redactor;
use shell::Shell;

async fn load_for_command(
    config: &Config,
    command: &str,
) -> Result<(
    HashMap<String, String>,
    HashMap<PathBuf, HashMap<String, String>>,
    HashMap<String, String>,
    FxHashSet<String>,
)> {
    let is_tty = std::io::stderr().is_terminal();
    prompt::confirm_disclaimers(&config.collect_disclaimers(command)).await?;
    let LoadedSecrets {
        vars,
        sources,
        maskable,
        warnings,
    } = hydration_or_exit(config, command).await;
    if is_tty && !warnings.is_empty() {
        message_box::MessageBox::new()
            .warning()
            .paragraphs(warnings.iter().map(String::as_str))
            .line("Waiting 5 seconds before continuing... (2x Ctrl-C to cancel)")
            .print_stderr();
        sleep_or_cancel(5).await;
    }
    compat::warn_outdated(compat::known_schemes(sources.values().map(|s| s.as_str()))).await;
    let (env, files) = split_env_files(vars);
    let mut names = write_files(&files)?;
    names.extend(env.keys().cloned());
    if is_tty && !names.is_empty() {
        names.sort();
        eprintln!("Lade loaded: {}.", names.join(", "));
        eprintln!();
    }
    Ok((env, files, sources, maskable))
}

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

    let is_tty = std::io::stderr().is_terminal();
    let upgrade_task = (is_tty && matches!(command, Command::Inject(_) | Command::Set(_)))
        .then(|| tokio::spawn(upgrade::check_message()));

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
        let value =
            hydrate_one(uri.clone(), &current_dir, &std::collections::HashMap::new()).await?;
        compat::warn_outdated(compat::known_schemes(std::iter::once(uri.as_str()))).await;
        println!("{}", value);
        return Ok(());
    }

    let config = match LadeFile::build(current_dir.clone()) {
        std::result::Result::Ok(c) => c,
        Err(e) => {
            message_box::MessageBox::new()
                .error()
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
            let (env, files, sources, maskable) = load_for_command(&config, &command).await?;
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
            let (env, ..) = load_for_command(&config, &command).await?;
            println!("{}", shell.set(env));
        }
        Command::Unset(EvalCommand { commands }) => {
            let command = commands.join(" ");
            let keys = config.collect_keys(&command);
            let (env, files) = split_env_files(keys);
            remove_files(&mut files.keys())?;
            println!("{}", shell.unset(env));
        }
        _ => unreachable!(),
    }

    if inject_exit_code != Some(130)
        && let Some(task) = upgrade_task
        && let Some(msg) = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .ok()
            .and_then(|r| r.ok())
            .and_then(|r| r.ok())
            .flatten()
    {
        message_box::MessageBox::new()
            .info()
            .line(msg)
            .line("Upgrade with: lade upgrade")
            .print_stderr();
        match prompt::ask_upgrade_choice().await {
            prompt::UpgradeChoice::Upgrade => upgrade::run_upgrade_subprocess()?,
            prompt::UpgradeChoice::Snooze(offset) => {
                upgrade::apply_snooze(offset).await.ok();
            }
            prompt::UpgradeChoice::Continue => {}
        }
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
