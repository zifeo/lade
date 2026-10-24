use anyhow::Result;
use std::env;

mod access;
mod agent_meta;
mod args;
mod audience;
mod cache;
mod catalog;
mod child_signals;
mod command;
mod compat;
mod config;
mod context;
mod event;
mod exit_codes;
mod family;
mod files;
mod global_config;
mod hub;
mod live_progress;
mod masking;
mod mcp;
mod message_box;
mod mise;
mod network;
mod packages;
mod preexec;
mod pretool;
mod prompt;
mod provider_progress;
mod redact;
mod scrub;
#[cfg(test)]
mod test_state;
mod ticket;
mod window;
mod wrap;

/// Home from the OS user database. `pretool::install::paths::home_dir`
/// prefers `$HOME`, so the two are not interchangeable.
pub(crate) fn home_dir() -> Option<std::path::PathBuf> {
    directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
}

use args::{Args, Command, DEFAULT_MASK_FORMAT, InjectCommand};
use clap::Parser;
use command::{run_config_verbs, run_standalone};
use config::LadeFile;
use context::InvocationContext;

pub fn cli_main() -> Result<()> {
    #[cfg(target_family = "unix")]
    {
        // fix the pipe: https://github.com/rust-lang/rust/issues/46016
        use nix::sys::signal;
        unsafe {
            signal::signal(signal::Signal::SIGPIPE, signal::SigHandler::SigDfl)?;
        }
    }

    // Spawned for every tunnel start: skip the runtime, logging, and clap.
    let argv = std::env::args_os().collect::<Vec<_>>();
    if argv.get(1).is_some_and(|verb| verb == network::GUARD_VERB) {
        network::run_guard(&argv[2..]);
    }

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<()> {
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

    if let Err(e) = cache::checked_root() {
        message_box::MessageBox::new()
            .error()
            .paragraph(e.to_string())
            .print_stderr();
        std::process::exit(exit_codes::FAILURE);
    }

    let pretool = args.pretool;
    let command = match args.command {
        Some(Command::InjectAlias(commands)) if commands.first().is_some_and(|c| c == "cache") => {
            message_box::MessageBox::new()
                .error()
                .paragraph("`lade cache` is now `lade ttl`. To run a program named cache, use `lade inject cache`.")
                .print_stderr();
            std::process::exit(exit_codes::FAILURE);
        }
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
            std::env::var(preexec::LADE_T)
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
    let Some(command) = run_standalone(command, &ctx).await? else {
        return Ok(());
    };

    let current_dir = env::current_dir()?;

    let config = match LadeFile::build(current_dir.clone()) {
        Ok(c) => c,
        Err(e) => {
            crate::config::report_load_error(&e);
            std::process::exit(exit_codes::FAILURE);
        }
    };

    let inject_exit_code = run_config_verbs(command, &ctx, &config, current_dir).await?;

    if let Some(code) = inject_exit_code {
        std::process::exit(code);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn verify_cli() {
        use crate::args::Args;
        use clap::CommandFactory;
        Args::command().debug_assert()
    }
}
