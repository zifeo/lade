use anyhow::Result;
use std::{env, io::Read, time::Duration};

mod access;
mod agent_meta;
mod args;
mod audience;
mod bench;
mod catalog;
mod child_signals;
mod compat;
mod config;
mod context;
mod event;
mod exec;
mod exit_codes;
mod files;
mod global_config;
mod inject;
mod log_cmd;
mod log_pack;
mod masking;
mod mcp;
mod message_box;
mod network;
mod pretool;
mod prompt;
mod provider_progress;
mod redact;
mod scrub;
mod shell;
mod status;
mod ticket;
mod upgrade;
mod user;
mod window;

use args::{Args, Command, DEFAULT_MASK_FORMAT, EvalCommand, InjectCommand};
use clap::Parser;
use config::{Config, LadeFile};
use context::InvocationContext;
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

    let (peeled_ticket_id, argv) = ticket::peel_pretool(std::env::args_os().collect());
    let args = Args::try_parse_from(&argv)?;

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
        args::print_command_help(
            &args.command,
            &event::db_path(),
            args::help_lists_internal(&args.verbose),
        )?;
        return Ok(());
    }

    let pretool = args.pretool;
    let command = match args.command {
        Some(Command::InjectAlias(commands)) => Command::Inject(InjectCommand {
            no_mask: false,
            mask_format: DEFAULT_MASK_FORMAT.to_string(),
            commands,
        }),
        Some(command) => command,
        None => {
            args::print_command_help(&None, &event::db_path(), false)?;
            return Ok(());
        }
    };
    let ticket_id = peeled_ticket_id.or_else(|| {
        if matches!(
            command,
            Command::Set(_) | Command::Unset(_) | Command::Approve { .. }
        ) {
            std::env::var(shell::LADE_T)
                .ok()
                .filter(|value| ticket::is_id(value))
        } else {
            None
        }
    });

    let ctx = match InvocationContext::from_command(&command, pretool, ticket_id) {
        Ok(ctx) => ctx,
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .paragraph(e.to_string())
                .print_stderr();
            std::process::exit(exit_codes::FAILURE);
        }
    };
    let upgrade_task = (ctx.is_interactive() && matches!(command, Command::Inject(_)))
        .then(|| tokio::spawn(upgrade::check_message()));

    let Some(command) = run_standalone(command, &ctx).await? else {
        return Ok(());
    };

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

    let inject_exit_code = run_config_verbs(command, &ctx, &config, current_dir).await?;

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

async fn run_standalone(command: Command, ctx: &InvocationContext) -> Result<Option<Command>> {
    match command {
        Command::On => {
            let shell = Shell::detect()?;
            println!("{}\n{}", shell.off()?, shell.on()?);
            Ok(None)
        }
        Command::Off => {
            let shell = Shell::detect()?;
            println!("{}", shell.off()?);
            Ok(None)
        }
        Command::Install => {
            let shell = Shell::detect()?;
            message_box::MessageBox::new()
                .info()
                .line(format!("Auto launcher installed in {}", shell.install()?))
                .print_plain_stderr();
            // Install is Quiet, so `is_interactive()` is false even on a TTY.
            let may_prompt = ctx.stdin_is_terminal && ctx.stderr_is_terminal;
            pretool::install::install(may_prompt)?;
            Ok(None)
        }
        Command::Uninstall => {
            let shell = Shell::detect()?;
            message_box::MessageBox::new()
                .info()
                .line(format!(
                    "Auto launcher uninstalled in {}",
                    shell.uninstall()?
                ))
                .print_plain_stderr();
            pretool::install::uninstall()?;
            Ok(None)
        }
        Command::Upgrade(opts) => upgrade::perform(opts).await.map(|()| None),
        Command::Status(opts) => status::run(opts).await.map(|()| None),
        Command::Log(opts) => {
            log_cmd::run_log(opts, ctx.audience == config::Audience::Agent).map(|()| None)
        }
        Command::Usage(opts) => log_cmd::run_usage(opts).map(|()| None),
        Command::Bench(opts) => bench::run(opts).await.map(|()| None),
        Command::User { username, reset } => user::run(username, reset).await.map(|()| None),
        other => Ok(Some(other)),
    }
}

async fn run_config_verbs(
    command: Command,
    ctx: &InvocationContext,
    config: &Config,
    current_dir: std::path::PathBuf,
) -> Result<Option<i32>> {
    match command {
        Command::Hook { harness } => {
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
            let output = pretool::handle(config, &input, ctx.audience, harness.as_deref())?;
            print!("{}", output);
            Ok(None)
        }
        Command::Inject(opts) => {
            if opts.commands.is_empty() {
                message_box::MessageBox::new()
                    .error()
                    .line("A command is required for `lade inject`.")
                    .print_stderr();
                std::process::exit(exit_codes::FAILURE);
            }
            let shell = Shell::detect()?;
            let command = opts.commands.join(" ");
            match map_disclaimer_exit(
                run_inject(command, opts, ctx, config, &shell, &current_dir).await,
            ) {
                Ok(code) => Ok(code),
                Err(e) => {
                    report_inject_error(&e);
                    std::process::exit(exit_codes::FAILURE);
                }
            }
        }
        Command::Mcp(opts) => {
            match map_disclaimer_exit(mcp::run(opts, ctx, config, &current_dir).await) {
                Ok(code) => Ok(code),
                Err(e) => {
                    report_inject_error(&e);
                    std::process::exit(exit_codes::FAILURE);
                }
            }
        }
        Command::Approve { code } => {
            let shell = Shell::detect()?;
            match map_disclaimer_exit(handle_approve(ctx, config, &shell, current_dir, code).await)
            {
                Ok(code) => Ok(code),
                Err(e) => {
                    report_inject_error(&e);
                    std::process::exit(exit_codes::FAILURE);
                }
            }
        }
        Command::Set(EvalCommand { commands }) => {
            let shell = Shell::detect()?;
            // Shell hooks are automatic but still part of an interactive
            // session. Check at most daily and deliberately discard the
            // result: stdout is the shell protocol and hook stderr must stay
            // quiet unless the hook itself has an access error.
            let _ = tokio::time::timeout(Duration::from_secs(2), upgrade::check_message()).await;
            handle_set(ctx, config, &shell, commands, current_dir).await?;
            Ok(None)
        }
        Command::Unset(EvalCommand { commands }) => {
            let shell = Shell::detect()?;
            handle_unset(ctx, &shell, config, commands).await?;
            Ok(None)
        }
        _ => unreachable!(),
    }
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
