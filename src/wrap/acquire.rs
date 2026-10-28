use anyhow::Result;
use std::future::Future;

use crate::config::{NetworkBinding, SecretSources};
use crate::context::InvocationContext;
use crate::files::{
    LoadedSecrets, hydrate_secrets_from_ticket_with_progress, remove_files, split_env_files,
    write_files,
};
use crate::network::Stop;
use crate::preexec::Shell;
use crate::provider_progress::{
    ProviderProgressSink, start_provider_progress, stop_provider_progress,
};
use crate::ticket::{self, TicketSecret};
use nix::sys::signal::{SigHandler, Signal};

use super::keys::{self, Keys};
use super::work::{CacheScope, SecretHydrate};
use super::{Acquisition, FailureChoice, SecretBundle, handle_provider_failure, log_phase};

/// Shared orchestration for `run_inject`/`handle_set`: starts the provider
/// progress renderer, hydrates secrets and acquires the network session
/// concurrently. `start_network` is generic over the session kind
/// (`network::start_attached_network_session` /
/// `network::start_detached_network_session`) so callers don't need a
/// runtime mode enum to get back the right session type.
///
/// Any failure goes through [`handle_provider_failure`]. `None`: a human
/// chose to run the command without Lade. Otherwise the command is
/// cancelled and Lade exits here.
pub(super) async fn acquire_secrets_and_network<N: Send + 'static>(
    ctx: &InvocationContext,
    secrets: SecretHydrate<'_>,
    network_bindings: Vec<NetworkBinding>,
    preexec: Option<&Shell>,
    start_network: impl FnOnce(&[NetworkBinding], ProviderProgressSink, &Stop) -> Result<N>
    + Send
    + 'static,
) -> Option<(SecretBundle, N)> {
    // A shell hook cannot keep the shell from running the command, so a
    // cancelled pre-exec command gets a PATH with nothing to run.
    let cancel_command = |code: i32| -> ! {
        if let Some(shell) = preexec {
            println!("{}", shell.block_command());
        }
        std::process::exit(code);
    };
    let provider_progress =
        start_provider_progress(ctx.stderr_is_terminal && !crate::message_box::terse());
    let secret_sink = provider_progress.sink();
    let network_sink = provider_progress.sink();
    let mut provider_progress = Some(provider_progress);
    // zsh and bash cancel the command on SIGINT themselves.
    let mut keys = matches!(preexec, Some(Shell::Fish))
        .then(|| Keys::enter(ctx))
        .flatten();

    let cancel = Stop::default();
    let network_cancel = cancel.clone();
    let mut network = tokio::task::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let session = start_network(&network_bindings, network_sink, &network_cancel);
        log_phase("tunnels", started);
        session
    });
    let acquisition = {
        let secret_task = prepare_secrets(
            secrets.secrets,
            secrets.op_sa,
            secrets.progress,
            secrets.cache,
            secret_sink,
        );
        let network_task = async {
            let result = (&mut network)
                .await
                .map_err(|e| anyhow::anyhow!("network task join error: {e}"))?;
            result.map_err(|e| anyhow::anyhow!("tunnel error: {e}"))
        };
        tokio::select! {
            acquisition = race_provider_tasks(secret_task, network_task) => Some(acquisition),
            () = interrupted() => None,
            () = keys::ctrl_c(&mut keys) => None,
        }
    };
    let Some(acquisition) = acquisition else {
        drop(keys);
        // Stop the workers, so the hub releases their tunnels before exit.
        cancel.stop();
        while !network.is_finished() {
            tokio::time::sleep(INTERRUPT_POLL).await;
        }
        drop(network);
        stop_provider_progress(&mut provider_progress);
        unlink_ticket_before_exit(secrets.ticket_unlink);
        cancel_command(crate::exit_codes::INTERRUPTED);
    };
    stop_provider_progress(&mut provider_progress);
    let error = match acquisition {
        Acquisition::Ready(secret_result, network_result) => {
            return Some((secret_result, network_result));
        }
        Acquisition::Failed(error) => error,
        Acquisition::FailedWithFiles(error, files) => {
            let _ = remove_files(&mut files.keys());
            error
        }
        Acquisition::FailedWithNetwork(error, network_result) => {
            drop(network_result);
            error
        }
    };
    let choice = handle_provider_failure(ctx, &error, keys).await;
    unlink_ticket_before_exit(secrets.ticket_unlink);
    match choice {
        FailureChoice::RunAnyway => None,
        FailureChoice::Cancel => cancel_command(crate::exit_codes::FAILURE),
        FailureChoice::Interrupted => cancel_command(crate::exit_codes::INTERRUPTED),
    }
}

const INTERRUPT_POLL: std::time::Duration = std::time::Duration::from_millis(10);

/// tokio keeps its handlers for the life of the process: once nothing
/// reads them, Ctrl-C, `kill`, and hangup must stop Lade again.
pub(super) fn restore_default_signals() {
    for signal in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGHUP] {
        let _ = unsafe { nix::sys::signal::signal(signal, SigHandler::SigDfl) };
    }
}

/// Ctrl-C, `kill`, or the terminal closing. Without a signal driver the
/// signals keep their default action.
pub(super) async fn interrupted() {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut interrupt), Ok(mut terminate), Ok(mut hangup)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
        signal(SignalKind::hangup()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = interrupt.recv() => {}
        _ = terminate.recv() => {}
        _ = hangup.recv() => {}
    }
}

fn unlink_ticket_before_exit(id: Option<&str>) {
    if let Some(id) = id {
        crate::hub::unlink_t(id);
        let _ = ticket::unlink(id);
    }
}

pub(super) async fn race_provider_tasks<N, S, T>(secret_task: S, network_task: T) -> Acquisition<N>
where
    S: Future<Output = Result<SecretBundle>>,
    T: Future<Output = Result<N>>,
{
    tokio::pin!(secret_task);
    tokio::pin!(network_task);

    tokio::select! {
        secret_result = &mut secret_task => {
            match secret_result {
                Ok(secret_result) => match network_task.await {
                    Ok(network_result) => Acquisition::Ready(secret_result, network_result),
                    Err(e) => {
                        let (_, files, ..) = secret_result;
                        Acquisition::FailedWithFiles(e, files)
                    }
                },
                Err(e) => {
                    let network_result = network_task.await;
                    match network_result {
                        Ok(network_result) => Acquisition::FailedWithNetwork(e, network_result),
                        Err(_) => Acquisition::Failed(e),
                    }
                }
            }
        }
        network_result = &mut network_task => {
            match network_result {
                Ok(network_result) => match secret_task.await {
                    Ok(secret_result) => Acquisition::Ready(secret_result, network_result),
                    Err(e) => Acquisition::FailedWithNetwork(e, network_result),
                },
                Err(e) => Acquisition::Failed(e),
            }
        }
    }
}

async fn prepare_secrets(
    secrets: &[TicketSecret],
    op_sa: Option<&str>,
    progress_plan: &SecretSources,
    cache: Option<CacheScope<'_>>,
    progress: ProviderProgressSink,
) -> Result<SecretBundle> {
    let started = std::time::Instant::now();
    let cache = cache.map(|scope| {
        crate::hub::Cache::open(scope.cwd, scope.walk, scope.saved_user, scope.patterned)
    });
    let hit = match cache.as_ref() {
        Some(cache) => cache.lookup(),
        None => crate::hub::CacheHit::empty(),
    };
    log_phase("hub lookup", started);
    let remaining: Vec<TicketSecret> = secrets
        .iter()
        .filter(|secret| !hit.keys.contains(&secret.key))
        .cloned()
        .collect();
    let LoadedSecrets {
        mut vars,
        mut sources,
        mut maskable,
        warnings,
    } = {
        let started = std::time::Instant::now();
        let loaded = hydrate_secrets_from_ticket_with_progress(
            &remaining,
            op_sa,
            progress_plan,
            progress,
            &hit.keys,
        )
        .await;
        log_phase("providers", started);
        loaded?
    };

    for secret in secrets {
        let Some(value) = hit.values.get(&secret.key) else {
            continue;
        };
        sources.insert(secret.key.clone(), secret.source.clone());
        maskable.insert(secret.key.clone());
        vars.entry(secret.output.clone())
            .or_default()
            .insert(secret.key.clone(), value.clone());
    }

    if let Some(cache) = cache.as_ref() {
        let started = std::time::Instant::now();
        let mut flat = std::collections::HashMap::new();
        for values in vars.values() {
            flat.extend(values.clone());
        }
        cache.store(&flat, &hit.keys);
        log_phase("hub store", started);
    }

    let (env, files) = split_env_files(vars);
    write_files(&files)?;
    Ok((env, files, sources, maskable, warnings, hit.keys))
}
