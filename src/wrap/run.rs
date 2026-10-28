use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::args::InjectCommand;
use crate::audience::Via;
use crate::config::Config;
use crate::context::InvocationContext;
use crate::event::{self, Diary, Kind};
use crate::files::remove_files;
use crate::masking;
use crate::network;
use crate::preexec::Shell;
use crate::prompt;
use crate::redact::Redactor;
use crate::wrap::exec;

use super::acquire::{acquire_secrets_and_network, restore_default_signals};
use super::pins::{PinCleanup, apply_pins, select_tool_env};
use super::work::{
    CacheScope, SecretHydrate, TicketCleanup, resolve_provider_work, rules_at, ticket_ready,
};
use super::{
    merge_env_with_conflicts, public_hydrate, seen_event, show_loader_warnings, work_event,
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
    let ticket_cleanup = if use_ticket && ctx.via != Via::Preexec {
        ctx.ticket_id
            .as_ref()
            .map(|id| TicketCleanup::new(id.clone()))
    } else {
        None
    };
    let (rules_dir, matched, moved) = rules_at(current_dir, &command)?;
    let config = moved.as_ref().unwrap_or(config);
    let saved_user = crate::config::saved_user().await?;
    let pins = apply_pins(config, matched, &rules_dir, &saved_user, ctx.audience, None).await?;
    let _pin_cleanup = PinCleanup(pins.cleanup.clone());
    let work = resolve_provider_work(
        config,
        matched,
        ctx.audience,
        ctx.ticket_id.as_deref(),
        use_ticket,
        &saved_user,
    )
    .await?;
    // Read once: a long command must not hold its ticket until it exits.
    drop(ticket_cleanup);
    let work = match work {
        Some(work) => work,
        None => {
            let diary = Diary::open(config.log_on_walk_for(ctx.audience), &rules_dir);

            return run_command_without_providers(
                &command,
                &opts,
                ctx,
                shell,
                current_dir,
                pins.env,
                || diary.write(|| seen_event(ctx, &command, &rules_dir, &saved_user, None)),
            );
        }
    };
    let diary = Diary::open(work.log, &rules_dir);
    if !work.needs_providers() {
        let row = work_event(
            ctx,
            &saved_user,
            rules_dir.clone(),
            command.clone(),
            work.agent,
            event::logged_kind(&work.matches),
            work.matches,
            None,
            None,
        );
        return run_command_without_providers(
            &command,
            &opts,
            ctx,
            shell,
            current_dir,
            pins.env,
            || diary.write(|| row),
        );
    }

    if let Err(e) = prompt::resolve_disclaimers(ctx, &work.disclaimers, &command).await {
        if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() {
            diary.write(|| {
                work_event(
                    ctx,
                    &saved_user,
                    rules_dir.clone(),
                    command.clone(),
                    work.agent.clone(),
                    Kind::Denied,
                    work.matches.clone(),
                    None,
                    None,
                )
            });
        }
        return Err(e);
    }
    // This wrap uses the hub: an agent may get the hourly notices to relay.
    if ctx.via == Via::Pretool {
        crate::command::notice::on_agent_use();
    }

    let hydrate_started = std::time::Instant::now();
    let ticket_unlink = (ctx.via == Via::Pretool)
        .then_some(ctx.ticket_id.as_deref())
        .flatten();
    let patterned = config.collect_for_with_pattern(matched, ctx.audience);
    let tunnel_cwd = rules_dir.clone();
    // Cache bodies are keyed by the rules this wrap read. A ticket from
    // other rules would read or store a value under the wrong body.
    let same_rules = work
        .cwd
        .as_ref()
        .is_none_or(|cwd| crate::pretool::same_dir(cwd, &rules_dir));
    let Some(((mut env, files, sources, maskable, warnings, cached), network)) =
        acquire_secrets_and_network(
            ctx,
            SecretHydrate {
                secrets: &work.secrets,
                op_sa: work.op_sa.as_deref(),
                progress: &work.progress,
                ticket_unlink,
                cache: same_rules.then_some(CacheScope {
                    cwd: &rules_dir,
                    walk: config.walk_hash(),
                    saved_user: &saved_user,
                    patterned: &patterned,
                }),
            },
            work.network_bindings,
            None,
            move |bindings, sink, cancel| {
                network::start_attached_network_session(bindings, sink, &tunnel_cwd, cancel)
            },
        )
        .await
    else {
        // The human chose to run it without Lade: no access to record.
        return run_command_without_providers(
            &command,
            &opts,
            ctx,
            shell,
            current_dir,
            HashMap::new(),
            || {},
        );
    };
    let started = std::time::Instant::now();
    let refused = show_loader_warnings(ctx, &warnings).await;
    super::log_phase("warnings", started);
    if refused {
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
    super::log_phase("hydrate total", hydrate_started);
    let hydrated = Some(public_hydrate(&env, &files));
    let mut matches = work.matches;
    event::mark_cached(&mut matches, &cached);
    let row = work_event(
        ctx,
        &saved_user,
        rules_dir.clone(),
        command.clone(),
        work.agent.clone(),
        event::logged_kind(&matches),
        matches,
        hydrated,
        hydrate_ms,
    );
    let code = exec::run(
        ctx,
        shell,
        &command,
        env.clone(),
        current_dir,
        redactor,
        || diary.write(|| row),
    );
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
    after_spawn: impl FnOnce(),
) -> Result<Option<i32>> {
    let redactor = if !opts.no_mask {
        Redactor::new(&HashMap::new(), &opts.mask_format)
    } else {
        None
    };
    let code = exec::run(ctx, shell, command, env, current_dir, redactor, after_spawn)?;
    Ok((code != 0).then_some(code))
}
