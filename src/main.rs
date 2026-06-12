use anyhow::{Ok, Result};
use std::{env, io::Read, time::Duration};

mod args;
mod compat;
mod config;
mod context;
mod exec;
mod files;
mod global_config;
mod hook;
mod inject;
mod masking;
mod message_box;
mod prompt;
mod redact;
mod shell;
mod status;
mod upgrade;

use args::{Args, Command, DEFAULT_MASK_FORMAT, EvalCommand, InjectCommand};
use clap::{CommandFactory, Parser};
use config::LadeFile;
use context::InvocationContext;
use global_config::GlobalConfig;
use inject::{handle_approve, handle_set, handle_unset, run_inject};
use lade_sdk::hydrate_one;
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
        Some(Command::InjectAlias(commands)) => Command::Inject(InjectCommand {
            no_mask: false,
            mask_format: DEFAULT_MASK_FORMAT.to_string(),
            commands,
        }),
        Some(command) => command,
        None => {
            Args::command().print_help()?;
            return Ok(());
        }
    };

    let ctx = InvocationContext::from_command(&command);
    let upgrade_task = (ctx.may_nudge() && matches!(command, Command::Inject(_)))
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
        Command::Status(opts) => return status::run(opts).await,
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
        if ctx.may_nudge() {
            compat::warn_outdated(&ctx, compat::known_schemes(std::iter::once(uri.as_str()))).await;
        }
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
            let command = opts.commands.join(" ");
            inject_exit_code =
                run_inject(command, opts, &ctx, &config, &shell, &current_dir).await?;
        }
        Command::Approve => {
            inject_exit_code = handle_approve(&ctx, &config, &shell, current_dir).await?;
        }
        Command::Set(EvalCommand { commands }) => {
            handle_set(&ctx, &config, &shell, commands, current_dir).await?;
        }
        Command::Unset(EvalCommand { commands }) => {
            handle_unset(&shell, &config, commands)?;
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
            .line("Run `lade upgrade` to update, or `lade status` for details.")
            .print_stderr();
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
