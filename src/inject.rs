use anyhow::Result;
use rustc_hash::FxHashSet;
use std::path::Path;
use std::{collections::HashMap, path::PathBuf};

use crate::args::InjectCommand;
use crate::config::{Config, LadeRule, NetworkBinding};
use crate::context::InvocationContext;
use crate::exec;
use crate::files::{
    LoadedSecrets, hydrate_secrets_with_progress, remove_files, sleep_or_cancel, split_env_files,
    write_files,
};
use crate::message_box;
use crate::network::{self, stop_network_pids};
use crate::prompt;
use crate::provider_progress::{
    ProviderProgressSink, start_provider_progress, stop_provider_progress,
};
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

type SecretBundle = (
    HashMap<String, String>,
    HashMap<PathBuf, HashMap<String, String>>,
    HashMap<String, String>,
    FxHashSet<String>,
    Vec<String>,
);

pub async fn run_inject(
    command: String,
    opts: InjectCommand,
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    current_dir: &Path,
) -> Result<Option<i32>> {
    let rules = config.collect(&command);
    if rules.is_empty() {
        return run_command_without_providers(&command, &opts, ctx, shell, current_dir);
    }

    let disclaimers = Config::disclaimers_from_rules(&rules);
    prompt::resolve_disclaimers(ctx, &disclaimers, &command).await?;

    let saved_user = crate::config::saved_user().await?;
    let network_bindings = Config::network_bindings_from_rules(&rules, &saved_user)?;
    let ((mut env, files, sources, maskable, warnings), network) = acquire_secrets_and_network(
        ctx,
        config,
        &rules,
        &saved_user,
        network_bindings,
        network::start_attached_network_session,
    )
    .await;
    show_loader_warnings(ctx, &warnings).await;
    merge_env_with_conflicts(&mut env, network.env.clone())?;
    compat::warn_outdated(
        ctx,
        compat::known_schemes(
            sources
                .values()
                .map(String::as_str)
                .chain(network.sources.iter().map(String::as_str)),
        ),
    )
    .await;
    let redactor = if !opts.no_mask {
        Redactor::new(
            &masking::secrets_for_redaction(&env, &files, &sources, &maskable),
            &opts.mask_format,
        )
    } else {
        None
    };
    let code = exec::run(ctx, shell.bin(), &command, env, current_dir, redactor);
    let code = match code {
        Ok(code) => {
            remove_files(&mut files.keys())?;
            code
        }
        Err(e) => {
            let _ = remove_files(&mut files.keys());
            drop(network);
            return Err(e);
        }
    };
    Ok((code != 0).then_some(code))
}

/// Fast path for a command that matches no rule at all: no disclaimer, no
/// secret, no network binding can apply, so skip straight to running the
/// command without spinning up the provider progress thread or any
/// secret/network machinery.
fn run_command_without_providers(
    command: &str,
    opts: &InjectCommand,
    ctx: &InvocationContext,
    shell: &Shell,
    current_dir: &Path,
) -> Result<Option<i32>> {
    let redactor = if !opts.no_mask {
        Redactor::new(&HashMap::new(), &opts.mask_format)
    } else {
        None
    };
    let code = exec::run(
        ctx,
        shell.bin(),
        command,
        HashMap::new(),
        current_dir,
        redactor,
    )?;
    Ok((code != 0).then_some(code))
}

/// Shared orchestration for `run_inject`/`handle_set`: starts the provider
/// progress renderer, hydrates secrets and acquires the network session
/// concurrently, and applies the same fail-closed handling to either side
/// failing. `start_network` is generic over the session kind
/// (`network::start_attached_network_session` /
/// `network::start_detached_network_session`) so callers don't need a
/// runtime mode enum to get back the right session type.
async fn acquire_secrets_and_network<N: Send + 'static>(
    ctx: &InvocationContext,
    config: &Config,
    rules: &[(PathBuf, LadeRule)],
    saved_user: &Option<String>,
    network_bindings: Vec<NetworkBinding>,
    start_network: impl FnOnce(&[NetworkBinding], ProviderProgressSink) -> Result<N> + Send + 'static,
) -> (SecretBundle, N) {
    let provider_progress = start_provider_progress(ctx.stderr_is_terminal);
    let secret_sink = provider_progress.sink();
    let network_sink = provider_progress.sink();
    let (secret_result, network_result) = tokio::join!(
        prepare_secrets(config, rules, saved_user, secret_sink),
        async {
            let result =
                tokio::task::spawn_blocking(move || start_network(&network_bindings, network_sink))
                    .await
                    .map_err(|e| anyhow::anyhow!("network task join error: {e}"))?;
            result.map_err(|e| anyhow::anyhow!("network provider error: {e}"))
        }
    );
    let mut provider_progress = Some(provider_progress);
    stop_provider_progress(&mut provider_progress);
    match (secret_result, network_result) {
        (Ok(secret_result), Ok(network_result)) => (secret_result, network_result),
        (Ok((_, files, ..)), Err(e)) => {
            let _ = remove_files(&mut files.keys());
            handle_provider_failure(ctx, &e).await;
            std::process::exit(crate::exit_codes::FAILURE);
        }
        (Err(e), Ok(network_result)) => {
            drop(network_result);
            handle_provider_failure(ctx, &e).await;
            std::process::exit(crate::exit_codes::FAILURE);
        }
        (Err(e), Err(_)) => {
            handle_provider_failure(ctx, &e).await;
            std::process::exit(crate::exit_codes::FAILURE);
        }
    }
}

async fn prepare_secrets(
    config: &Config,
    rules: &[(PathBuf, LadeRule)],
    saved_user: &Option<String>,
    progress: ProviderProgressSink,
) -> Result<SecretBundle> {
    let LoadedSecrets {
        vars,
        sources,
        maskable,
        warnings,
    } = hydrate_secrets_with_progress(config, rules, saved_user, progress).await?;

    let (env, files) = split_env_files(vars);
    write_files(&files)?;
    Ok((env, files, sources, maskable, warnings))
}

pub async fn handle_set(
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    commands: Vec<String>,
    current_dir: PathBuf,
) -> Result<()> {
    println!(
        "{};{}",
        shell.clear_pending_line(),
        shell.clear_network_line()
    );
    let command = commands.join(" ");
    let rules = config.collect(&command);
    if rules.is_empty() {
        println!("{}", shell.set(HashMap::new()));
        return Ok(());
    }

    let disclaimers = Config::disclaimers_from_rules(&rules);
    if let Err(e) = prompt::resolve_disclaimers(ctx, &disclaimers, &command).await {
        if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() {
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
        return Err(e);
    }
    let saved_user = crate::config::saved_user().await?;
    let network_bindings = match Config::network_bindings_from_rules(&rules, &saved_user) {
        Ok(bindings) => bindings,
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .line("Lade could not resolve network providers:")
                .paragraph(e.to_string())
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    let ((mut env, _files, _sources, _maskable, warnings), detached) = acquire_secrets_and_network(
        ctx,
        config,
        &rules,
        &saved_user,
        network_bindings,
        network::start_detached_network_session,
    )
    .await;
    show_loader_warnings(ctx, &warnings).await;
    merge_env_with_conflicts(&mut env, detached.env)?;
    if !detached.pids.is_empty() {
        let raw = detached
            .pids
            .into_iter()
            .map(|pid| pid.to_string())
            .collect::<Vec<_>>()
            .join(",");
        env.insert(crate::shell::LADE_NETWORK_PIDS.to_string(), raw);
    }
    println!("{}", shell.set(env));
    Ok(())
}

async fn handle_provider_failure(ctx: &InvocationContext, e: &anyhow::Error) {
    if e.to_string().contains("network provider") {
        message_box::MessageBox::new()
            .error()
            .line("Lade could not start network providers:")
            .paragraph(e.to_string())
            .print_stderr();
        return;
    }
    handle_loader_failure(ctx, e).await;
}

async fn handle_loader_failure(ctx: &InvocationContext, e: &anyhow::Error) {
    let mut mb = loader_error_box(e);
    if ctx.is_interactive() {
        mb = mb.line("Waiting 5 seconds before continuing... (2x Ctrl-C to cancel)");
    }
    mb.print_stderr();
    if ctx.is_interactive() {
        sleep_or_cancel(5).await;
    }
}

async fn show_loader_warnings(ctx: &InvocationContext, warnings: &[String]) {
    if warnings.is_empty() {
        return;
    }
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

fn merge_env_with_conflicts(
    env: &mut HashMap<String, String>,
    incoming: HashMap<String, String>,
) -> Result<()> {
    for (key, value) in incoming {
        match env.get(&key) {
            Some(existing) if existing != &value => {
                anyhow::bail!(
                    "conflicting env '{}' between secret/network providers: '{}' vs '{}'",
                    key,
                    existing,
                    value
                );
            }
            Some(_) => {}
            None => {
                env.insert(key, value);
            }
        }
    }
    Ok(())
}

pub async fn handle_unset(shell: &Shell, config: &Config, commands: Vec<String>) -> Result<()> {
    if let Ok(raw) = std::env::var(crate::shell::LADE_NETWORK_PIDS) {
        stop_network_pids(&raw);
    }
    let command = commands.join(" ");
    let rules = config.collect(&command);
    let keys = if rules.is_empty() {
        HashMap::new()
    } else {
        let saved_user = crate::config::saved_user().await?;
        Config::keys_from_rules(&rules, &saved_user)
    };
    let (env, files) = split_env_files(keys);
    remove_files(&mut files.keys())?;
    let mut keys = env;
    keys.push(crate::shell::LADE_NETWORK_PIDS.to_string());
    println!("{}", shell.unset(keys));
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
