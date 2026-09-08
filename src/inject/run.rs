use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::args::InjectCommand;
use crate::audience::Via;
use crate::compat;
use crate::config::Config;
use crate::context::InvocationContext;
use crate::event::{self, Emit, Kind};
use crate::exec;
use crate::files::remove_files;
use crate::masking;
use crate::network;
use crate::prompt;
use crate::redact::Redactor;
use crate::shell::Shell;

use super::acquire::acquire_secrets_and_network;
use super::{
    PinCleanup, SecretHydrate, TicketCleanup, apply_pins, emit_seen_if_walk_log,
    merge_env_with_conflicts, public_hydrate, resolve_provider_work, select_tool_env,
    show_loader_warnings, ticket_ready,
};

pub async fn run_inject(
    command: String,
    opts: InjectCommand,
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    current_dir: &Path,
) -> Result<Option<i32>> {
    let use_ticket = ticket_ready(ctx.ticket_id.as_deref());
    let _ticket_cleanup = if use_ticket && ctx.via != Via::Preexec {
        ctx.ticket_id
            .as_ref()
            .map(|id| TicketCleanup::new(id.clone()))
    } else {
        None
    };
    let saved_user = crate::config::saved_user().await?;
    let pins = apply_pins(config, &command, current_dir, &saved_user).await?;
    let _pin_cleanup = PinCleanup(pins.cleanup.clone());
    let work = resolve_provider_work(
        config,
        &command,
        ctx.audience,
        ctx.ticket_id.as_deref(),
        use_ticket,
        &saved_user,
    )
    .await?;
    let work = match work {
        Some(work) => work,
        None => {
            emit_seen_if_walk_log(config, ctx, &command, current_dir, &saved_user, None);
            return run_command_without_providers(
                &command,
                &opts,
                ctx,
                shell,
                current_dir,
                pins.env,
            );
        }
    };

    if let Err(e) = prompt::resolve_disclaimers(ctx, &work.disclaimers, &command).await {
        if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() {
            event::emit_if(
                work.log,
                Emit {
                    kind: Kind::Denied,
                    via: ctx.via,
                    audience: ctx.audience,
                    actor: event::actor(&saved_user),
                    cwd: current_dir.to_path_buf(),
                    command: command.clone(),
                    argv: None,
                    hydrated: None,
                    matches: work.matches.clone(),
                    hydrate_ms: None,
                    agent: crate::agent_meta::merge(work.agent.clone()),
                },
            );
        }
        return Err(e);
    }

    let hydrate_started = std::time::Instant::now();
    let ticket_unlink = (ctx.via == Via::Pretool)
        .then_some(ctx.ticket_id.as_deref())
        .flatten();
    let ((mut env, files, sources, maskable, warnings), network) = acquire_secrets_and_network(
        ctx,
        SecretHydrate {
            secrets: &work.secrets,
            op_sa: work.op_sa.as_deref(),
            progress: &work.progress,
            ticket_unlink,
        },
        work.network_bindings,
        work.log,
        network::start_attached_network_session,
    )
    .await;
    show_loader_warnings(ctx, &warnings).await;
    if let Err(error) = merge_env_with_conflicts(&mut env, network.env.clone()) {
        let _ = remove_files(&mut files.keys());
        drop(network);
        return Err(error);
    }
    select_tool_env(&mut env, pins.env)?;
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
    let hydrate_ms = Some(hydrate_started.elapsed().as_secs_f64() * 1000.0);
    let hydrated = Some(public_hydrate(&env, &files));
    event::emit_if(
        work.log,
        Emit {
            kind: event::logged_kind(&work.matches),
            via: ctx.via,
            audience: ctx.audience,
            actor: event::actor(&saved_user),
            cwd: current_dir.to_path_buf(),
            command: command.clone(),
            argv: None,
            hydrated,
            matches: work.matches,
            hydrate_ms,
            agent: crate::agent_meta::merge(work.agent.clone()),
        },
    );
    let code = exec::run(ctx, shell, &command, env.clone(), current_dir, redactor);
    let _ = remove_files(&mut files.keys());
    match code {
        Ok(code) => {
            drop(network);
            Ok((code != 0).then_some(code))
        }
        Err(e) => {
            drop(network);
            Err(e)
        }
    }
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
    env: HashMap<String, String>,
) -> Result<Option<i32>> {
    let redactor = if !opts.no_mask {
        Redactor::new(&HashMap::new(), &opts.mask_format)
    } else {
        None
    };
    let code = exec::run(ctx, shell, command, env, current_dir, redactor)?;
    Ok((code != 0).then_some(code))
}
