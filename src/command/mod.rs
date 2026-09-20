pub(crate) mod add;
mod bench;
mod eval;
mod status;
mod upgrade;
mod user;

use anyhow::Result;
use std::io::Read;
use std::time::Duration;

use crate::args;
use crate::args::{Command, EvalCommand, HookAction, HookTarget};
use crate::config::Config;
use crate::context::InvocationContext;
use crate::event;
use crate::event::log_cmd;
use crate::exit_codes;
use crate::mcp;
use crate::message_box::MessageBox;
use crate::preexec::Shell;
use crate::pretool;
use crate::prompt;
use crate::wrap::{handle_approve, handle_set, handle_unset, run_inject};

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
        Command::Setup(opts) => {
            let mode = if opts.unlock {
                crate::mise::PinMode::Unlock
            } else {
                crate::mise::PinMode::Locked
            };
            run_setup(ctx, &opts.slugs(), mode).await?;
            Ok(None)
        }
        Command::Update => {
            require_lade_yaml()?;
            crate::mise::setup_pins(crate::mise::PinMode::Update).await?;
            Ok(None)
        }
        Command::Teardown => {
            require_lade_yaml()?;
            let tool = pretool::install::teardown()?;
            crate::mise::run_lifecycle_commands("teardown").await?;
            crate::packages::run("teardown").await?;
            pretool::install::print_teardown(&tool);
            Ok(None)
        }
        Command::Add(opts) => {
            add::run_add(opts, ctx)?;
            run_setup(ctx, &[], crate::mise::PinMode::Locked).await?;
            Ok(None)
        }
        Command::Remove(opts) => {
            add::run_remove(opts, ctx)?;
            Ok(None)
        }
        Command::Upgrade(opts) => upgrade::perform(opts).await.map(|()| None),
        Command::Eval {
            uri,
            access_command,
        } => {
            if access_command.as_ref().is_some_and(|name| name.is_empty()) {
                anyhow::bail!("--access-command cannot be empty");
            }
            let command = access_command.unwrap_or_else(|| "eval".to_string());
            let current_dir = std::env::current_dir()?;
            let value =
                eval::resolve_uri(uri, &current_dir, &command, ctx.via, ctx.audience).await?;
            println!("{value}");
            Ok(None)
        }
        Command::Hook {
            action: Some(action),
            ..
        } => {
            match action {
                HookAction::Enable(opts) => match opts.target()? {
                    HookTarget::Shell => {
                        let (pre, reload) = crate::preexec::enable_current_preexec()?;
                        pretool::install::print_shell_hook(
                            &pre.found,
                            pre.verb,
                            &pre.path,
                            reload.as_deref(),
                        );
                    }
                    HookTarget::Agent(agent) => {
                        pretool::install::install_scoped(hook_scope(opts.scope), agent.slug())?;
                    }
                },
                HookAction::Disable(opts) => match opts.target()? {
                    HookTarget::Shell => {
                        let pre = crate::preexec::uninstall_current_preexec()?;
                        pretool::install::print_shell_hook(&pre.found, pre.verb, &pre.path, None);
                    }
                    HookTarget::Agent(agent) => {
                        pretool::install::uninstall_scoped(hook_scope(opts.scope), agent.slug())?;
                    }
                },
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
                    .line("`lade hook` reads pre-tool JSON on stdin.")
                    .line("Pipe a payload, or let an agent invoke it.")
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

async fn run_setup(
    ctx: &InvocationContext,
    slugs: &[&str],
    mode: crate::mise::PinMode,
) -> Result<()> {
    let shell = crate::preexec::maybe_bootstrap_setup_shell()?;
    let may_prompt = ctx.stdin_is_terminal
        && ctx.stderr_is_terminal
        && ctx.audience == crate::config::Audience::Human;
    let cwd = std::env::current_dir()?;
    let snap = crate::mise::scan(&cwd);
    if snap.project_git_root.is_none() {
        let mut mb = MessageBox::new()
            .info()
            .line("This folder is not a git repo.");
        if crate::preexec::ci_job() {
            mb = mb.line("CI. No shell wrap. Repo bins, locks, and agent hooks are skipped.");
        } else {
            mb = mb
                .line("Shell wrap only. Repo bins, locks, and agent hooks are skipped.")
                .line("cd into a repo and run `lade setup`.");
        }
        mb.print_stderr();
        if snap.is_mise() {
            require_lade_yaml()?;
            crate::mise::setup_pins(mode).await?;
        }
        let tool = pretool::install::setup(may_prompt, slugs)?;
        pretool::install::print_setup(&shell, &tool);
        return Ok(());
    }
    require_lade_yaml()?;
    crate::mise::setup_pins(mode).await?;
    let tool = pretool::install::setup(may_prompt, slugs)?;
    crate::mise::run_lifecycle_commands("setup").await?;
    crate::packages::run("setup").await?;
    pretool::install::print_setup(&shell, &tool);
    Ok(())
}

fn require_lade_yaml() -> Result<()> {
    let cwd = std::env::current_dir()?;
    if let Err(e) = crate::config::LadeFile::build(cwd) {
        crate::config::report_load_error(&e);
        std::process::exit(exit_codes::FAILURE);
    }
    Ok(())
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
        .line("Could not prepare the command.")
        .line("")
        .paragraph(e.to_string())
        .line("")
        .line("Check the provider URI, token, or CLI.")
        .print_stderr();
}
