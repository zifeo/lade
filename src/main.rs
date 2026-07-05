use anyhow::Result;
use std::{env, io::Read, time::Duration};

mod agent;
mod agent_hooks;
mod args;
mod compat;
mod config;
mod context;
mod exec;
mod exit_codes;
mod files;
mod global_config;
mod hook;
mod inject;
mod masking;
mod message_box;
mod network;
mod prompt;
mod provider_progress;
mod provider_registry;
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
    let upgrade_task = (ctx.is_interactive() && matches!(command, Command::Inject(_)))
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
            message_box::MessageBox::new()
                .info()
                .line(format!("Auto launcher installed in {}", shell.install()?))
                .print_plain_stderr();
            // Computed here, not from `ctx.is_interactive()`: Install maps to Hook mode,
            // so `is_interactive()` is always false even on a real terminal.
            let may_prompt = ctx.stdin_is_terminal && ctx.stderr_is_terminal;
            agent_hooks::install(may_prompt)?;
            return Ok(());
        }
        Command::Uninstall => {
            message_box::MessageBox::new()
                .info()
                .line(format!(
                    "Auto launcher uninstalled in {}",
                    shell.uninstall()?
                ))
                .print_plain_stderr();
            agent_hooks::uninstall()?;
            return Ok(());
        }
        Command::Upgrade(opts) => return upgrade::perform(opts).await,
        Command::Status(opts) => return status::run(opts).await,
        Command::User { username, reset } => {
            if reset {
                GlobalConfig::update(|c| c.user = None).await?;
                message_box::MessageBox::new()
                    .info()
                    .line("Successfully reset lade user")
                    .print_plain_stderr();
                return Ok(());
            }
            if let Some(user) = username {
                if user.is_empty() {
                    message_box::MessageBox::new()
                        .error()
                        .line("No user provided.")
                        .print_stderr();
                    std::process::exit(exit_codes::FAILURE);
                }
                GlobalConfig::update(|c| c.user = Some(user.clone())).await?;
                message_box::MessageBox::new()
                    .info()
                    .line(format!("Successfully set user to {user}"))
                    .print_plain_stderr();
                return Ok(());
            }
            let config = GlobalConfig::load().await?;
            if let Some(user) = config.user {
                println!("{}", user);
            } else {
                message_box::MessageBox::new()
                    .info()
                    .line("No user set. Lade will use the current OS user.")
                    .print_plain_stderr();
            }
            return Ok(());
        }
        _ => {}
    }

    let current_dir = env::current_dir()?;

    if let Command::Eval { uri } = command {
        let value =
            hydrate_one(uri.clone(), &current_dir, &std::collections::HashMap::new()).await?;
        if ctx.is_interactive() {
            compat::warn_outdated(&ctx, compat::known_schemes(std::iter::once(uri.as_str()))).await;
        }
        println!("{}", value);
        return Ok(());
    }

    let config = match LadeFile::build(current_dir.clone()) {
        Ok(c) => c,
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .line("Lade could not parse a config file:")
                .line("")
                .paragraph(e.to_string())
                .line("")
                .line("Hint: check the file format.")
                .print_stderr();
            std::process::exit(exit_codes::FAILURE);
        }
    };

    let mut inject_exit_code: Option<i32> = None;

    match command {
        Command::Hook => {
            if ctx.stdin_is_terminal {
                message_box::MessageBox::new()
                    .error()
                    .line("`lade hook` is meant to be invoked automatically by AI agents.")
                    .line("")
                    .line(
                        "It reads a JSON payload from stdin. To use it manually, pipe JSON into it.",
                    )
                    .print_stderr();
                std::process::exit(exit_codes::FAILURE);
            }
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let output = hook::handle(&config, &input)?;
            print!("{}", output);
        }
        Command::Inject(opts) => {
            let command = opts.commands.join(" ");
            inject_exit_code = match map_disclaimer_exit(
                run_inject(command, opts, &ctx, &config, &shell, &current_dir).await,
            ) {
                Ok(code) => code,
                Err(e) => {
                    report_inject_error(&e);
                    std::process::exit(exit_codes::FAILURE);
                }
            };
        }
        Command::Approve { code } => {
            inject_exit_code = match map_disclaimer_exit(
                handle_approve(&ctx, &config, &shell, current_dir, code).await,
            ) {
                Ok(code) => code,
                Err(e) => {
                    report_inject_error(&e);
                    std::process::exit(exit_codes::FAILURE);
                }
            };
        }
        Command::Set(EvalCommand { commands }) => {
            handle_set(&ctx, &config, &shell, commands, current_dir).await?;
        }
        Command::Unset(EvalCommand { commands }) => {
            handle_unset(&shell, &config, commands).await?;
        }
        _ => unreachable!(),
    }

    if inject_exit_code != Some(exit_codes::INTERRUPTED)
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

/// Translate a withheld-disclaimer error (already reported to the user) into
/// the dedicated [`exit_codes::DISCLAIMER_WITHHELD`] code, leaving every other
/// result untouched so genuine errors still bubble up to `main`.
fn map_disclaimer_exit(result: Result<Option<i32>>) -> Result<Option<i32>> {
    match result {
        Err(e) if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() => {
            Ok(Some(exit_codes::DISCLAIMER_WITHHELD))
        }
        other => other,
    }
}

fn report_inject_error(e: &anyhow::Error) {
    message_box::MessageBox::new()
        .error()
        .line("Lade could not prepare command execution:")
        .line("")
        .paragraph(e.to_string())
        .line("")
        .line("Hint: verify provider URI format and local CLI access.")
        .print_stderr();
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
