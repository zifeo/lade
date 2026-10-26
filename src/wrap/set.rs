use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::context::InvocationContext;
use crate::event::{self, Kind};
use crate::files::remove_files;
use crate::hub::VersionLedger;
use crate::message_box::MessageBox;
use crate::network;
use crate::preexec::Shell;
use crate::prompt;
use crate::ticket;

use super::acquire::{acquire_secrets_and_network, restore_default_signals};
use super::pins::{apply_pins, select_tool_env};
use super::work::{
    CacheScope, SecretHydrate, pre_event_from_work, resolve_provider_work, ticket_ready,
};
use super::{
    emit_seen_if_walk_log, merge_env_with_conflicts, public_hydrate, show_loader_warnings,
    work_event,
};

/// `notice`: ask the hub for the hourly notice slot, only when this command
/// talks to it anyway. `Some`: this shell owns the slot.
pub async fn handle_set(
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    commands: Vec<String>,
    current_dir: PathBuf,
    notice: bool,
) -> Result<Option<VersionLedger>> {
    println!(
        "{}",
        shell.unset(vec![crate::preexec::LADE_RESTORE.to_string()])
    );
    match set_env(ctx, config, shell, commands, current_dir, notice).await {
        Ok(slot) => Ok(slot),
        Err(error) => {
            MessageBox::new()
                .error()
                .paragraph(error.to_string())
                .print_stderr();
            super::refuse_preexec(shell, crate::exit_codes::FAILURE);
        }
    }
}

async fn set_env(
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    commands: Vec<String>,
    current_dir: PathBuf,
    notice: bool,
) -> Result<Option<VersionLedger>> {
    let command = commands.join(" ");
    let use_ticket = ticket_ready(ctx.ticket_id.as_deref());
    let saved_user = crate::config::saved_user().await?;
    let original_path = std::env::var("PATH").ok();
    let pins = apply_pins(
        config,
        &command,
        &current_dir,
        &saved_user,
        ctx.audience,
        Some(shell),
    )
    .await?;
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
            emit_seen_if_walk_log(config, ctx, &command, &current_dir, &saved_user, None);
            if pins.is_empty() {
                println!("{}", shell.set(HashMap::new()));
                return Ok(None);
            }
            return stamp_pins_and_return(
                ctx,
                shell,
                pins.env,
                ticket::PreEvent::new(
                    &command,
                    current_dir.clone(),
                    ctx.via,
                    ctx.audience,
                    event::actor(&saved_user),
                ),
                original_path,
                notice,
            );
        }
    };
    if !work.needs_providers() {
        event::emit_if(
            work.log,
            work_event(
                ctx,
                &saved_user,
                current_dir.clone(),
                command.clone(),
                work.agent,
                event::logged_kind(&work.matches),
                work.matches,
                None,
                None,
            ),
        );
        if pins.is_empty() {
            println!("{}", shell.set(HashMap::new()));
            return Ok(None);
        }
        return stamp_pins_and_return(
            ctx,
            shell,
            pins.env,
            ticket::PreEvent::new(
                &command,
                current_dir.clone(),
                ctx.via,
                ctx.audience,
                event::actor(&saved_user),
            ),
            original_path,
            notice,
        );
    }

    let mut pre = pre_event_from_work(
        &command,
        current_dir.clone(),
        ctx.via,
        ctx.audience,
        event::actor(&saved_user),
        &work,
    );
    let id = ticket::write_or_replace(ctx.ticket_id.as_deref(), &pre)?;
    let slot = put_first_t(&id, notice);

    if let Err(e) = prompt::resolve_disclaimers(ctx, &work.disclaimers, &command).await {
        if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() {
            event::emit_if(
                work.log,
                work_event(
                    ctx,
                    &saved_user,
                    current_dir.clone(),
                    command.clone(),
                    work.agent.clone(),
                    Kind::Denied,
                    work.matches.clone(),
                    None,
                    None,
                ),
            );
            pre.pending = true;
            ticket::replace(&id, &pre)?;
            crate::hub::put_t(&id);
            println!(
                "{}",
                shell.set(HashMap::from([(crate::preexec::LADE_T.to_string(), id)]))
            );
            std::process::exit(crate::exit_codes::DISCLAIMER_WITHHELD);
        }
        crate::hub::unlink_t(&id);
        let _ = ticket::unlink(&id);
        return Err(e);
    }
    let hydrate_started = std::time::Instant::now();
    let patterned = config.collect_for_with_pattern(&command, ctx.audience);
    let tunnel_cwd = current_dir.clone();
    let Some(((mut env, files, _sources, _maskable, warnings, cached), detached)) =
        acquire_secrets_and_network(
            ctx,
            SecretHydrate {
                secrets: &work.secrets,
                op_sa: work.op_sa.as_deref(),
                progress: &work.progress,
                ticket_unlink: Some(id.as_str()),
                cache: Some(CacheScope {
                    cwd: &current_dir,
                    walk: config.walk_hash(),
                    saved_user: &saved_user,
                    patterned: &patterned,
                }),
            },
            work.network_bindings,
            work.log,
            Some(shell),
            move |bindings, sink, cancel| {
                network::start_detached_network_session(bindings, sink, &tunnel_cwd, cancel)
            },
        )
        .await
    else {
        // The human chose to run it without Lade: the shell gets nothing.
        return Ok(slot);
    };
    let started = std::time::Instant::now();
    let refused = show_loader_warnings(ctx, &warnings).await;
    super::log_phase("warnings", started);
    if refused {
        let _ = remove_files(&mut files.keys());
        crate::hub::unlink_t(&id);
        let _ = ticket::unlink(&id);
        super::refuse_preexec(shell, crate::exit_codes::INTERRUPTED);
    }
    restore_default_signals();
    let finish = || -> Result<String> {
        merge_env_with_conflicts(&mut env, detached.env.clone())?;
        select_tool_env(&mut env, pins.env)?;
        pre.pending = false;
        let started = std::time::Instant::now();
        ticket::replace(&id, &pre)?;
        crate::hub::put_t(&id);
        super::log_phase("ticket", started);
        let mut matches = work.matches;
        event::mark_cached(&mut matches, &cached);
        event::emit_if(
            work.log,
            work_event(
                ctx,
                &saved_user,
                current_dir.clone(),
                command.clone(),
                work.agent.clone(),
                event::logged_kind(&matches),
                matches,
                Some(public_hydrate(&env, &files)),
                Some(hydrate_started.elapsed().as_secs_f64() * 1000.0),
            ),
        );
        super::log_phase("hydrate total", hydrate_started);
        let line = stamp_preexec(shell, env, &id, original_path)?;
        // The shell gets LADE_T from this line, so `lade unset` can release.
        detached.hand_to_shell(&id)?;
        Ok(line)
    };
    match finish() {
        Ok(line) => {
            println!("{line}");
            Ok(slot)
        }
        // No LADE_T reaches the shell, so `lade unset` never runs for it.
        Err(e) => {
            let _ = remove_files(&mut files.keys());
            crate::hub::unlink_t(&id);
            let _ = ticket::unlink(&id);
            Err(e)
        }
    }
}

fn put_first_t(id: &str, notice: bool) -> Option<VersionLedger> {
    if notice {
        return crate::hub::put_t_notice(id);
    }
    crate::hub::put_t(id);
    None
}

/// Pins only: a ticket with no provider work, so a later `lade unset` still
/// finds a file to release.
fn stamp_pins_and_return(
    ctx: &InvocationContext,
    shell: &Shell,
    pins_env: HashMap<String, String>,
    pre: ticket::PreEvent,
    original_path: Option<String>,
    notice: bool,
) -> Result<Option<VersionLedger>> {
    let id = ticket::write_or_replace(ctx.ticket_id.as_deref(), &pre)?;
    let slot = put_first_t(&id, notice);
    println!("{}", stamp_preexec(shell, pins_env, &id, original_path)?);
    Ok(slot)
}

fn stamp_preexec(
    shell: &Shell,
    mut env: HashMap<String, String>,
    ticket_id: &str,
    original_path: Option<String>,
) -> Result<String> {
    env.insert(crate::preexec::LADE_T.to_string(), ticket_id.to_string());
    let mut previous = env
        .keys()
        .map(|key| {
            let value = if key == "PATH" {
                original_path.clone()
            } else {
                std::env::var(key).ok()
            };
            (key.clone(), value)
        })
        .collect::<HashMap<_, _>>();
    previous.insert("MISE_ENV".to_string(), std::env::var("MISE_ENV").ok());
    previous.insert(
        "MISE_SETTINGS".to_string(),
        std::env::var("MISE_SETTINGS").ok(),
    );
    env.insert(
        crate::preexec::LADE_RESTORE.to_string(),
        crate::preexec::RestorePayload { env: previous }.encode()?,
    );
    let set = shell.set(env);
    let clear = shell.unset(vec!["MISE_ENV".to_string(), "MISE_SETTINGS".to_string()]);
    Ok(format!("{set};{clear}"))
}
