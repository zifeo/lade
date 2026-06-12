use anyhow::{Context, Ok, Result, bail};
use rustc_hash::FxHashSet;
use std::path::Path;
use std::{collections::HashMap, path::PathBuf};

use crate::args::InjectCommand;
use crate::config::Config;
use crate::context::InvocationContext;
use crate::exec;
use crate::files::{
    LoadedSecrets, hydrate_secrets, remove_files, sleep_or_cancel, split_env_files, write_files,
};
use crate::message_box;
use crate::prompt;
use crate::redact::Redactor;
use crate::shell::Shell;
use crate::{compat, masking};

pub async fn run_inject(
    command: String,
    opts: InjectCommand,
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    current_dir: &Path,
) -> Result<Option<i32>> {
    let (env, files, sources, maskable) = prepare_secrets(ctx, config, &command).await?;
    let redactor = if !opts.no_mask {
        Redactor::new(
            &masking::secrets_for_redaction(&env, &files, &sources, &maskable),
            &opts.mask_format,
        )
    } else {
        None
    };
    let code = exec::run(shell.bin(), &command, env, current_dir, redactor);
    remove_files(&mut files.keys())?;
    let code = code?;
    Ok((code != 0).then_some(code))
}

pub async fn prepare_secrets(
    ctx: &InvocationContext,
    config: &Config,
    command: &str,
) -> Result<(
    HashMap<String, String>,
    HashMap<PathBuf, HashMap<String, String>>,
    HashMap<String, String>,
    FxHashSet<String>,
)> {
    prompt::resolve_disclaimers(ctx, config, command).await?;

    let LoadedSecrets {
        vars,
        sources,
        maskable,
        warnings,
    } = match hydrate_secrets(config, command).await {
        std::result::Result::Ok(secrets) => secrets,
        std::result::Result::Err(e) => {
            let mut mb = message_box::MessageBox::new()
                .error()
                .line("Lade could not get secrets from one loader:")
                .paragraph(e.to_string())
                .line("Hint: check whether the loader is connected to the correct vault.");
            if ctx.may_nudge() {
                mb = mb.line("Waiting 5 seconds before continuing... (2x Ctrl-C to cancel)");
            }
            mb.print_stderr();
            if ctx.may_nudge() {
                sleep_or_cancel(5).await;
            }
            std::process::exit(1);
        }
    };

    if !warnings.is_empty() {
        let mut mb = message_box::MessageBox::new()
            .warning()
            .paragraphs(warnings.iter().map(String::as_str));
        if ctx.may_nudge() {
            mb = mb.line("Waiting 5 seconds before continuing... (2x Ctrl-C to cancel)");
        }
        mb.print_stderr();
        if ctx.may_nudge() {
            sleep_or_cancel(5).await;
        }
    }

    compat::warn_outdated(
        ctx,
        compat::known_schemes(sources.values().map(|s| s.as_str())),
    )
    .await;

    let (env, files) = split_env_files(vars);
    let mut names = write_files(&files)?;
    names.extend(env.keys().cloned());
    if ctx.may_nudge() && !names.is_empty() {
        names.sort();
        message_box::MessageBox::new()
            .info()
            .line(format!("Lade loaded: {}.", names.join(", ")))
            .print_stderr();
    }
    Ok((env, files, sources, maskable))
}

pub async fn handle_set(
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    commands: Vec<String>,
    current_dir: PathBuf,
) -> Result<()> {
    println!("{}", shell.clear_pending_line());
    let command = commands.join(" ");
    match prepare_secrets(ctx, config, &command).await {
        std::result::Result::Ok((env, ..)) => {
            println!("{}", shell.set(env));
        }
        std::result::Result::Err(e) => {
            let disclaimers = config.collect_disclaimers(&command);
            if !disclaimers.is_empty() && !prompt::is_approved(&disclaimers) {
                let pending = crate::shell::PendingPayload {
                    cmd: command,
                    cwd: current_dir,
                };
                println!(
                    "{}",
                    shell.set(HashMap::from([(
                        crate::shell::LADE_PENDING.to_string(),
                        pending.encode()?
                    )]))
                );
            }
            message_box::MessageBox::new()
                .error()
                .line("Lade could not get secrets from one loader:")
                .paragraph(e.to_string())
                .line("Hint: check whether the loader is connected to the correct vault.")
                .print_stderr();
            std::process::exit(1);
        }
    }
    Ok(())
}

pub fn handle_unset(shell: &Shell, config: &Config, commands: Vec<String>) -> Result<()> {
    let command = commands.join(" ");
    let keys = config.collect_keys(&command);
    let (env, files) = split_env_files(keys);
    remove_files(&mut files.keys())?;
    println!("{}", shell.unset(env));
    Ok(())
}

pub async fn handle_approve(
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    current_dir: PathBuf,
) -> Result<Option<i32>> {
    let pending_env =
        std::env::var(crate::shell::LADE_PENDING).context("no pending disclaimer to approve")?;
    let pending = crate::shell::PendingPayload::decode(&pending_env)?;
    if pending.cwd != current_dir {
        bail!(
            "pending disclaimer was for a different directory: {}",
            pending.cwd.display()
        );
    }
    let opts = InjectCommand {
        no_mask: false,
        mask_format: crate::args::DEFAULT_MASK_FORMAT.to_string(),
        commands: vec![],
    };
    // lade approve acts as the interactive "yes" for the pending command
    unsafe {
        std::env::set_var(crate::shell::LADE_ACCEPT_DISCLAIMER, "1");
    }
    run_inject(pending.cmd, opts, ctx, config, shell, &current_dir).await
}
