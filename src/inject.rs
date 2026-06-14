use anyhow::Result;
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

fn loader_error_box(e: &anyhow::Error) -> message_box::MessageBox {
    message_box::MessageBox::new()
        .error()
        .line("Lade could not get secrets from one loader:")
        .paragraph(e.to_string())
        .line("Hint: check whether the loader is connected to the correct vault.")
}

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
    let code = exec::run(ctx, shell.bin(), &command, env, current_dir, redactor);
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
        Ok(secrets) => secrets,
        Err(e) => {
            let mut mb = loader_error_box(&e);
            if ctx.is_interactive() {
                mb = mb.line("Waiting 5 seconds before continuing... (2x Ctrl-C to cancel)");
            }
            mb.print_stderr();
            if ctx.is_interactive() {
                sleep_or_cancel(5).await;
            }
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };

    if !warnings.is_empty() {
        let mut mb = message_box::MessageBox::new()
            .warning()
            .paragraphs(warnings.iter().map(String::as_str));
        if ctx.is_interactive() {
            mb = mb.line("Waiting 5 seconds before continuing... (2x Ctrl-C to cancel)");
        }
        mb.print_stderr();
        if ctx.is_interactive() {
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
    if ctx.stderr_is_terminal {
        message_box::print_loaded_message(names);
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
        Ok((env, ..)) => {
            println!("{}", shell.set(env));
        }
        Err(e) if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() => {
            // resolve_disclaimers already printed the disclaimer box; record the
            // pending command so `lade approve` can replay it, then exit without
            // a second (loader-shaped) message.
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
            std::process::exit(crate::exit_codes::DISCLAIMER_WITHHELD);
        }
        Err(e) => {
            loader_error_box(&e).print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
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
    code: Option<String>,
) -> Result<Option<i32>> {
    let code = match code {
        Some(c) => c,
        None => {
            message_box::MessageBox::new()
                .error()
                .line("Run `lade approve <code>` with the code shown in the disclaimer.")
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    let pending_env = match std::env::var(crate::shell::LADE_PENDING) {
        Ok(v) => v,
        Err(_) => {
            message_box::MessageBox::new()
                .error()
                .line("Nothing to approve: no disclaimer is pending.")
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    let pending = match crate::shell::PendingPayload::decode(&pending_env) {
        Ok(p) => p,
        Err(_) => {
            message_box::MessageBox::new()
                .error()
                .line("The pending disclaimer state is corrupted. Re-run the command.")
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    if pending.cwd != current_dir {
        message_box::MessageBox::new()
            .error()
            .line("The pending disclaimer was for a different directory:")
            .paragraph(pending.cwd.display().to_string())
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    }
    if !prompt::verify_code(&pending.cmd, &code) {
        message_box::MessageBox::new()
            .error()
            .line("Wrong or expired approval code. Re-run the command for a fresh one.")
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    }
    let opts = InjectCommand {
        no_mask: false,
        mask_format: crate::args::DEFAULT_MASK_FORMAT.to_string(),
        commands: vec![],
    };
    // The code is verified, so let resolve_disclaimers through for this command.
    unsafe {
        std::env::set_var(crate::shell::LADE_APPROVE, code);
    }
    run_inject(pending.cmd, opts, ctx, config, shell, &current_dir).await
}
