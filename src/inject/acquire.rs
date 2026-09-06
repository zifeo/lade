use anyhow::Result;
use std::future::Future;

use crate::config::{NetworkBinding, SecretSources};
use crate::context::InvocationContext;
use crate::event;
use crate::files::{
    LoadedSecrets, hydrate_secrets_from_ticket_with_progress, remove_files, split_env_files,
    write_files,
};
use crate::provider_progress::{
    ProviderProgressSink, start_provider_progress, stop_provider_progress,
};
use crate::ticket::{self, TicketSecret};

use super::{Acquisition, SecretBundle, SecretHydrate, handle_provider_failure};

/// Shared orchestration for `run_inject`/`handle_set`: starts the provider
/// progress renderer, hydrates secrets and acquires the network session
/// concurrently, and applies the same fail-closed handling to either side
/// failing. `start_network` is generic over the session kind
/// (`network::start_attached_network_session` /
/// `network::start_detached_network_session`) so callers don't need a
/// runtime mode enum to get back the right session type.
pub(super) async fn acquire_secrets_and_network<N: Send + 'static>(
    ctx: &InvocationContext,
    secrets: SecretHydrate<'_>,
    network_bindings: Vec<NetworkBinding>,
    warmup: bool,
    start_network: impl FnOnce(&[NetworkBinding], ProviderProgressSink) -> Result<N> + Send + 'static,
) -> (SecretBundle, N) {
    let provider_progress = start_provider_progress(ctx.stderr_is_terminal);
    let secret_sink = provider_progress.sink();
    let network_sink = provider_progress.sink();
    let mut provider_progress = Some(provider_progress);
    let warmup = warmup.then(|| tokio::task::spawn_blocking(event::warmup));

    let acquisition = {
        let secret_task = prepare_secrets(
            secrets.secrets,
            secrets.op_sa,
            secrets.progress,
            secret_sink,
        );
        let network_task = async {
            let result =
                tokio::task::spawn_blocking(move || start_network(&network_bindings, network_sink))
                    .await
                    .map_err(|e| anyhow::anyhow!("network task join error: {e}"))?;
            result.map_err(|e| anyhow::anyhow!("network provider error: {e}"))
        };
        race_provider_tasks(secret_task, network_task).await
    };

    if let Some(warmup) = warmup {
        let _ = warmup.await;
    }
    stop_provider_progress(&mut provider_progress);
    match acquisition {
        Acquisition::Ready(secret_result, network_result) => (secret_result, network_result),
        Acquisition::Failed(e) => {
            handle_provider_failure(ctx, &e).await;
            unlink_ticket_before_exit(secrets.ticket_unlink);
            std::process::exit(crate::exit_codes::FAILURE);
        }
        Acquisition::FailedWithFiles(e, files) => {
            let _ = remove_files(&mut files.keys());
            handle_provider_failure(ctx, &e).await;
            unlink_ticket_before_exit(secrets.ticket_unlink);
            std::process::exit(crate::exit_codes::FAILURE);
        }
        Acquisition::FailedWithNetwork(e, network_result) => {
            drop(network_result);
            handle_provider_failure(ctx, &e).await;
            unlink_ticket_before_exit(secrets.ticket_unlink);
            std::process::exit(crate::exit_codes::FAILURE);
        }
    }
}

fn unlink_ticket_before_exit(id: Option<&str>) {
    if let Some(id) = id {
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
    progress: ProviderProgressSink,
) -> Result<SecretBundle> {
    let LoadedSecrets {
        vars,
        sources,
        maskable,
        warnings,
    } = hydrate_secrets_from_ticket_with_progress(secrets, op_sa, progress_plan, progress).await?;

    let (env, files) = split_env_files(vars);
    write_files(&files)?;
    Ok((env, files, sources, maskable, warnings))
}
