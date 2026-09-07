use anyhow::Result;
use std::io::Read;
use std::time::Duration;

use crate::args::{Command, EvalCommand, HookAction};
use crate::config::Config;
use crate::context::InvocationContext;
use crate::exit_codes;
use crate::inject::{handle_approve, handle_set, handle_unset, run_inject};
use crate::message_box::MessageBox;
use crate::pretool;
use crate::prompt;
use crate::shell::Shell;
use crate::{args, bench, event, log_cmd, mcp, status, upgrade, user};

pub(crate) async fn run_standalone(
    command: Command,
    ctx: &InvocationContext,
) -> Result<Option<Command>> {
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
        Command::Install(opts) => {
            let shell = Shell::detect()?;
            MessageBox::new()
                .info()
                .line(format!("Auto launcher installed in {}", shell.install()?))
                .print_plain_stderr();
            // Install is Quiet, so `is_interactive()` is false even on a TTY.
            let may_prompt = ctx.stdin_is_terminal && ctx.stderr_is_terminal;
            pretool::install::install(may_prompt, &opts.slugs())?;
            Ok(None)
        }
        Command::Uninstall => {
            let shell = Shell::detect()?;
            MessageBox::new()
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
        Command::Hook {
            action: Some(action),
            ..
        } => {
            match action {
                HookAction::Install(opts) => {
                    pretool::install::install_scoped(hook_scope(opts.scope), opts.harness.slug())?;
                }
                HookAction::Uninstall(opts) => {
                    pretool::install::uninstall_scoped(
                        hook_scope(opts.scope),
                        opts.harness.slug(),
                    )?;
                }
            }
            Ok(None)
        }
        Command::Hook { action: None, .. } if ctx.stdin_is_terminal => {
            args::print_command_help(
                &Some(Command::Hook {
                    harness: None,
                    action: None,
                }),
                &event::db_path(),
                false,
            )?;
            Ok(None)
        }
        Command::Status(opts) => status::run(opts).await.map(|()| None),
        Command::Log(opts) => {
            log_cmd::run_log(opts, ctx.audience == crate::config::Audience::Agent).map(|()| None)
        }
        Command::Usage(opts) => log_cmd::run_usage(opts).map(|()| None),
        Command::Bench(opts) => bench::run(opts).await.map(|()| None),
        Command::User { username, reset } => user::run(username, reset).await.map(|()| None),
        other => Ok(Some(other)),
    }
}

pub(crate) async fn run_config_verbs(
    command: Command,
    ctx: &InvocationContext,
    config: &Config,
    current_dir: std::path::PathBuf,
) -> Result<Option<i32>> {
    match command {
        Command::Hook {
            harness,
            action: None,
        } => {
            if ctx.stdin_is_terminal {
                MessageBox::new()
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
                MessageBox::new()
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

fn hook_scope(scope: args::HookScope) -> pretool::install::Scope {
    match scope {
        args::HookScope::User => pretool::install::Scope::User,
        args::HookScope::Project => pretool::install::Scope::Project,
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
    MessageBox::new()
        .error()
        .line("Lade could not prepare command execution:")
        .line("")
        .paragraph(e.to_string())
        .line("")
        .line("Hint: verify provider URI format and local CLI access.")
        .print_stderr();
}
