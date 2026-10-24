use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::args::InjectCommand;
use crate::audience::Via;
use crate::config::Config;
use crate::context::InvocationContext;
use crate::event::{self, Kind};
use crate::files::remove_files;
use crate::masking;
use crate::network;
use crate::preexec::Shell;
use crate::prompt;
use crate::redact::Redactor;
use crate::wrap::exec;

use super::acquire::{acquire_secrets_and_network, restore_default_signals};
use super::pins::{PinCleanup, apply_pins, select_tool_env};
use super::work::{CacheScope, SecretHydrate, TicketCleanup, resolve_provider_work, ticket_ready};
use super::{
    emit_seen_if_walk_log, merge_env_with_conflicts, public_hydrate, show_loader_warnings,
    work_event,
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
    let pins = apply_pins(
        config,
        &command,
        current_dir,
        &saved_user,
        ctx.audience,
        None,
    )
    .await?;
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
    if !work.needs_providers() {
        event::emit_if(
            work.log,
            work_event(
                ctx,
                &saved_user,
                current_dir.to_path_buf(),
                command.clone(),
                work.agent,
                event::logged_kind(&work.matches),
                work.matches,
                None,
                None,
            ),
        );
        return run_command_without_providers(&command, &opts, ctx, shell, current_dir, pins.env);
    }

    if let Err(e) = prompt::resolve_disclaimers(ctx, &work.disclaimers, &command).await {
        if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() {
            event::emit_if(
                work.log,
                work_event(
                    ctx,
                    &saved_user,
                    current_dir.to_path_buf(),
                    command.clone(),
                    work.agent.clone(),
                    Kind::Denied,
                    work.matches.clone(),
                    None,
                    None,
                ),
            );
        }
        return Err(e);
    }

    let hydrate_started = std::time::Instant::now();
    let ticket_unlink = (ctx.via == Via::Pretool)
        .then_some(ctx.ticket_id.as_deref())
        .flatten();
    let patterned = config.collect_for_with_pattern(&command, ctx.audience);
    let tunnel_cwd = current_dir.to_path_buf();
    let Some(((mut env, files, sources, maskable, warnings, cached), network)) =
        acquire_secrets_and_network(
            ctx,
            SecretHydrate {
                secrets: &work.secrets,
                op_sa: work.op_sa.as_deref(),
                progress: &work.progress,
                ticket_unlink,
                cache: Some(CacheScope {
                    cwd: current_dir,
                    walk: config.walk_hash(),
                    saved_user: &saved_user,
                    patterned: &patterned,
                }),
            },
            work.network_bindings,
            work.log,
            None,
            move |bindings, sink, cancel| {
                network::start_attached_network_session(bindings, sink, &tunnel_cwd, cancel)
            },
        )
        .await
    else {
        // The human chose to run it without Lade.
        return run_command_without_providers(
            &command,
            &opts,
            ctx,
            shell,
            current_dir,
            HashMap::new(),
        );
    };
    if show_loader_warnings(ctx, &warnings).await {
        let _ = remove_files(&mut files.keys());
        drop(network);
        std::process::exit(crate::exit_codes::INTERRUPTED);
    }
    restore_default_signals();
    if let Err(error) = merge_env_with_conflicts(&mut env, network.env.clone()) {
        let _ = remove_files(&mut files.keys());
        drop(network);
        return Err(error);
    }
    select_tool_env(&mut env, pins.env)?;
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
    let mut matches = work.matches;
    event::mark_cached(&mut matches, &cached);
    event::emit_if(
        work.log,
        work_event(
            ctx,
            &saved_user,
            current_dir.to_path_buf(),
            command.clone(),
            work.agent.clone(),
            event::logged_kind(&matches),
            matches,
            hydrated,
            hydrate_ms,
        ),
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

/// Fast path when nothing is injected: no disclaimer, no secret, no
/// network binding. Skip the provider progress thread and hydrate.
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
