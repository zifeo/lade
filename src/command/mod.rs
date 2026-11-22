pub(crate) mod add;
mod bench;
mod eval;
pub(crate) mod notice;
mod setup;
mod status;
mod ttl;
pub(crate) mod upgrade;
mod user;

use anyhow::Result;
use std::io::Read;

use crate::args;
use crate::args::{Command, EvalCommand, HookAction, HookTarget};
use crate::audience::Via;
use crate::config::Config;
use crate::context::InvocationContext;
use crate::event;
use crate::event::log_cmd;
use crate::exit_codes;
use crate::mcp;
use crate::message_box::{MessageBox, Report};
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
        // The ticket holds all it undoes, and the snapshot comes from the
        // shell: a `lade.yaml` broken while the command ran still restores.
        Command::Unset(EvalCommand { commands }) => {
            let shell = Shell::detect()?;
            handle_unset(ctx, &shell, commands)?;
            Ok(None)
        }
        Command::Setup(opts) => {
            let mode = if opts.unlock {
                crate::mise::PinMode::Unlock
            } else {
                crate::mise::PinMode::Locked
            };
            setup::run_setup(ctx, &opts.slugs(), mode).await?;
            Ok(None)
        }
        Command::Update => {
            setup::require_lade_yaml()?;
            let bumped;
            {
                let _progress = crate::live_progress::begin(ctx.stderr_is_terminal);
                bumped = crate::mise::setup_pins(crate::mise::PinMode::Update).await?;
            }
            match bumped {
                report if report.is_empty() => Report::new().line("No pins in this repo.").print(),
                report => crate::mise::print_pin_update(&report.bumped),
            }
            upgrade::nudge_if_available();
            Ok(None)
        }
        Command::Teardown(opts) => {
            if opts.global {
                crate::cache::clear_and_report()?;
                return Ok(None);
            }
            setup::require_lade_yaml()?;
            // § ep7nzy2xg.a@y46ft
            let tool = pretool::install::teardown()?;
            crate::mise::run_lifecycle_commands("teardown").await?;
            crate::packages::run("teardown").await?;
            // § end ep7nzy2xg.a
            pretool::install::print_teardown(&tool);
            Ok(None)
        }
        Command::Add(opts) => {
            // § qp8tmdhae@7ts5s
            add::run_add(opts, ctx)?;
            setup::run_setup(ctx, &[], crate::mise::PinMode::Locked).await?;
            Ok(None)
        }
        Command::Remove(opts) => {
            // § qp8tmdhae@d31dv
            add::run_remove(opts, ctx)?;
            Ok(None)
        }
        // § one bvr6wk206.c@z01d9
        Command::Upgrade(opts) => upgrade::perform(opts).await.map(|()| None),
        Command::Eval {
            uri,
            access_command,
            ttl,
        } => {
            let Some(uri) = uri else {
                anyhow::bail!("the secret URI is required");
            };
            if access_command.as_ref().is_some_and(|name| name.is_empty()) {
                anyhow::bail!("--access-command cannot be empty");
            }
            // § y8w7xhrr1.a@m78ac
            let ttl = ttl
                .map(|raw| {
                    crate::config::RuleTtl::parse(&raw).map_err(|e| anyhow::anyhow!("--ttl: {e}"))
                })
                .transpose()?;
            // § end y8w7xhrr1.a
            let command = access_command.unwrap_or_else(|| "eval".to_string());
            let current_dir = std::env::current_dir()?;
            let value = eval::resolve_uri(
                uri,
                &current_dir,
                &command,
                ctx.via,
                ctx.audience,
                ttl.as_ref(),
            )
            .await?;
            // § one xrjs58t2q.a@q20t6
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
                            pre.verb,
                            &pre.path,
                            reload.as_deref(),
                            pretool::install::PREEXEC_SETUP_DIM,
                        );
                    }
                    HookTarget::Agent(agent) => {
                        pretool::install::install_scoped(hook_scope(opts.global), agent.slug())?;
                    }
                },
                HookAction::Disable(opts) => match opts.target()? {
                    HookTarget::Shell => {
                        let pre = crate::preexec::uninstall_current_preexec()?;
                        pretool::install::print_shell_hook(
                            pre.verb,
                            &pre.path,
                            None,
                            pretool::install::PREEXEC_DISABLE_DIM,
                        );
                    }
                    HookTarget::Agent(agent) => {
                        pretool::install::uninstall_scoped(hook_scope(opts.global), agent.slug())?;
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
        Command::Connect(opts) => {
            let path = opts
                .path
                .ok_or_else(|| anyhow::anyhow!("lade connect needs a path on the remote"))?;
            crate::remote::connect::run(ctx, path, opts.transport).await?;
            Ok(None)
        }
        Command::Detach(opts) => crate::remote::connect::detach(opts.transport).map(|()| None),
        Command::Attach(opts) => {
            let laptop = crate::remote::link::decode_arg(&opts.laptop)?;
            let transport = crate::remote::link::decode_arg(&opts.transport)?;
            std::process::exit(crate::remote::attach::run(laptop, transport, opts.resume).await?);
        }
        Command::Remote => std::process::exit(crate::remote::link::run().await?),
        Command::LinkedTunnel(opts) => std::process::exit(
            crate::remote::tunnel::run(&opts.uri, &opts.walk, &opts.listen).await?,
        ),
        Command::RemoteFiles(opts) => crate::remote::files::run(&opts.path).await.map(|()| None),
        Command::Ttl(opts) => ttl::run(opts).map(|()| None),
        Command::Log(opts) => {
            log_cmd::run_log(opts, ctx.audience == crate::config::Audience::Agent).map(|()| None)
        }
        Command::Usage(opts) => log_cmd::run_usage(opts).map(|()| None),
        Command::Bench(opts) => bench::run(opts).await.map(|()| None),
        Command::User { username, reset } => user::run(username, reset).await.map(|()| None),
        Command::Hub => {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                // Killed outright, the hub would leave its tunnels'
                // providers running.
                // § vdbmtzqtb.d@5kxhb
                let mut term = signal(SignalKind::terminate())?;
                let mut interrupt = signal(SignalKind::interrupt())?;
                crate::hub::serve(async move {
                    tokio::select! {
                        _ = term.recv() => {}
                        _ = interrupt.recv() => {}
                    }
                })
                .await?;
            }
            Ok(None)
        }
        other => Ok(Some(other)),
    }
}

pub(crate) async fn run_config_verbs(
    command: Command,
    ctx: &InvocationContext,
    config: &Config,
    current_dir: std::path::PathBuf,
    preexec_shell: Option<Shell>,
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
                    .line("Pipe a payload, or let a harness invoke it.")
                    .print_stderr();
                std::process::exit(exit_codes::FAILURE);
            }
            let input = hook_input()?;
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
            let command = opts.commands.join(" ");
            let shell = run_shell(ctx)?;
            finish_wrapped(map_disclaimer_exit(
                run_inject(command, opts, ctx, config, &shell, &current_dir).await,
            ))
        }
        Command::Mcp(opts) => finish_wrapped(map_disclaimer_exit(
            mcp::run(opts, ctx, config, &current_dir).await,
        )),
        Command::Approve { code } => {
            let shell = run_shell(ctx)?;
            finish_wrapped(map_disclaimer_exit(
                handle_approve(ctx, config, &shell, current_dir, code).await,
            ))
        }
        Command::Set(EvalCommand { commands }) => {
            let Some(shell) = preexec_shell else {
                unreachable!("the pre-exec shell is detected before the yaml")
            };
            let notice = notice::shell_notice(ctx);
            let command = commands.join(" ");
            let slot = handle_set(ctx, Ok(config), &shell, commands, current_dir, notice).await?;
            if notice {
                notice::on_shell_use(&command, slot);
            }
            Ok(None)
        }
        _ => unreachable!(),
    }
}

/// The walk did not load: a command Lade would run is refused on every door.
pub(crate) async fn run_unloaded(
    command: Command,
    ctx: &InvocationContext,
    error: &anyhow::Error,
    current_dir: std::path::PathBuf,
    preexec_shell: Option<Shell>,
) -> Result<()> {
    // § y4zs3xgg4.d@rq62z
    match command {
        Command::Set(EvalCommand { commands }) => {
            let Some(shell) = preexec_shell else {
                unreachable!("the pre-exec shell is detected before the yaml")
            };
            handle_set(ctx, Err(error), &shell, commands, current_dir, false).await?;
            Ok(())
        }
        Command::Hook {
            harness,
            action: None,
        } if !ctx.stdin_is_terminal => {
            let input = hook_input()?;
            print!(
                "{}",
                pretool::handle_unloaded(error, &input, harness.as_deref())
            );
            Ok(())
        }
        _ => {
            crate::config::report_load_error(error);
            std::process::exit(exit_codes::FAILURE);
        }
    }
}

fn hook_input() -> Result<String> {
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes)?;
    // § one f7eb9ymg0.a@wrwgg
    // A payload that is not UTF-8 cannot be JSON: hand it on as unread, so
    // the hook allows the tool call with its warning instead of failing.
    let input = String::from_utf8(bytes).unwrap_or_default();
    Ok(input)
}

/// The shell the command was written for: the one that started Lade. A
/// task runner (make, npm, just) runs its line with `sh -c`, which execs
/// Lade and leaves the runner as its parent: such a line was written for
/// `sh`. A pre-tool line was written for the harness shell, which must be
/// the parent.
fn run_shell(ctx: &InvocationContext) -> Result<Shell> {
    let error = match Shell::detect() {
        Ok(shell) => return Ok(shell),
        Err(e) => e,
    };
    if ctx.via != Via::Pretool {
        // § dpky46ff1.a@j5rde
        return Ok(Shell::Sh);
    }
    // § dpky46ff1.d@bvj3w
    MessageBox::new()
        .error()
        .line("A pre-tool command runs in the shell that started Lade.")
        .line("")
        .paragraph(format!("{error:#}."))
        .line("")
        .line("Run it from bash, zsh, fish, or sh. The pre-tool hook")
        .line("ends its rewrite with `&& true` so the shell stays.")
        .print_stderr();
    std::process::exit(exit_codes::FAILURE);
}

fn hook_scope(global: bool) -> pretool::install::Scope {
    if global {
        pretool::install::Scope::User
    } else {
        pretool::install::Scope::Project
    }
}

/// Translate a withheld-disclaimer error (already reported to the user) into
/// the dedicated [`exit_codes::DISCLAIMER_WITHHELD`] code, leaving every other
/// result untouched so genuine errors still bubble up to `main`.
fn map_disclaimer_exit(result: Result<Option<i32>>) -> Result<Option<i32>> {
    // § betd87z6r.b@s991m
    match result {
        Err(e) if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() => {
            // § tk1e8j9nf.a@nc8kx
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

fn finish_wrapped(result: Result<Option<i32>>) -> Result<Option<i32>> {
    match result {
        Ok(code) => Ok(code),
        Err(e) => {
            report_inject_error(&e);
            std::process::exit(exit_codes::FAILURE);
        }
    }
}
