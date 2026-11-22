use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::context::InvocationContext;
use crate::event::{self, Diary, Kind};
use crate::files::remove_files;
use crate::hub::VersionLedger;
use crate::message_box::MessageBox;
use crate::network;
use crate::preexec::Shell;
use crate::prompt;
use crate::ticket;

use super::acquire::{acquire_secrets_and_network, preexec_keys, restore_default_signals};
use super::keys;
use super::pins::{apply_pins, select_tool_env};
use super::work::{
    CacheScope, SecretHydrate, pre_event_from_work, resolve_provider_work, rules_at, ticket_ready,
};
use super::{
    emit_seen_if_walk_log, merge_env_with_conflicts, scrub_values, show_loader_warnings, work_event,
};

/// The pre-exec hook runs this exact line unwrapped, so it still works while
/// every other line is cancelled.
const OFF_WAY_OUT: &str = "Fix the file, or pause Lade in this shell: eval \"$(lade off)\"";

/// `notice`: ask the hub for the hourly notice slot, only when this command
/// talks to it anyway. `Some`: this shell owns the slot. `config` is the
/// walk's load error when it did not load.
pub async fn handle_set(
    ctx: &InvocationContext,
    config: std::result::Result<&Config, &anyhow::Error>,
    shell: &Shell,
    commands: Vec<String>,
    current_dir: PathBuf,
    notice: bool,
) -> Result<Option<VersionLedger>> {
    // § gy20cgb72.e@3xbq2
    // A pending ticket is this shell's withheld line. Any other ticket in
    // `LADE_T` came down from an outer shell's command, which still runs.
    let pending = ctx
        .ticket_id
        .as_deref()
        .filter(|id| ticket::read(id).is_ok_and(|pre| pre.pending));
    let mut unset = vec![crate::preexec::LADE_RESTORE.to_string()];
    if ctx.ticket_id.is_some() && pending.is_none() {
        unset.push(crate::preexec::LADE_T.to_string());
    }
    let ctx = &InvocationContext {
        ticket_id: pending.map(str::to_string),
        ..ctx.clone()
    };
    // § end gy20cgb72.e
    // § one qmybbcmkx.a@cn57s
    println!("{}", shell.unset(unset));
    let config = match config {
        Ok(config) => config,
        Err(error) => {
            // § y4zs3xgg4.d@0rfk0
            // § one y4zs3xgg4.f@q8tf1
            crate::config::load_error_box(error)
                .line("")
                .line(OFF_WAY_OUT)
                .print_stderr();
            // silcrow:follow
            super::refuse_preexec(shell, crate::exit_codes::FAILURE);
        }
    };
    match set_env(ctx, config, shell, commands, current_dir, notice).await {
        Ok(slot) => Ok(slot),
        Err(error) => {
            // § t7kcndtra.a@chdv1
            MessageBox::new()
                .error()
                .paragraph(error.to_string())
                .print_stderr();
            // silcrow:follow
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
    let (current_dir, matched, moved) = rules_at(&current_dir, &command)?;
    let config = moved.as_ref().unwrap_or(config);
    let use_ticket = ticket_ready(ctx.ticket_id.as_deref(), &command);
    let saved_user = crate::config::saved_user().await?;
    let original_path = std::env::var("PATH").ok();
    // § s6j22qtcr.b@typvt
    let mut keys = preexec_keys(ctx, Some(shell));
    let pins = tokio::select! {
        pins = apply_pins(
            config,
            matched,
            &current_dir,
            &saved_user,
            ctx.audience,
            Some(shell),
        ) => Some(pins?),
        () = keys::ctrl_c(&mut keys) => None,
    };
    let Some(pins) = pins else {
        drop(keys);
        super::refuse_preexec(shell, crate::exit_codes::INTERRUPTED);
    };
    // § end s6j22qtcr.b
    let work = resolve_provider_work(
        config,
        matched,
        ctx.audience,
        ctx.ticket_id.as_deref(),
        use_ticket,
        &saved_user,
    )
    .await?;
    let work = match work {
        Some(work) => work,
        None => {
            // § mzce8bpjn.b@44y2w
            emit_seen_if_walk_log(config, ctx, &command, &current_dir, &saved_user, None);
            if pins.is_empty() {
                // § f18xcggkd.a@7fxfk
                println!("{}", shell.set(HashMap::new()));
                return Ok(None);
            }
            // § end mzce8bpjn.b
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
        // § mzce8bpjn.b@txqmv
        event::emit_if(
            work.log,
            work_event(
                ctx,
                &saved_user,
                current_dir.clone(),
                command.clone(),
                work.agent,
                event::logged_kind(!pins.is_empty()),
                work.matches,
                None,
                None,
            ),
        );
        if pins.is_empty() {
            // § f18xcggkd.a@75k1y
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

    let diary = Diary::open(work.log, &current_dir);
    // § one bvttr6qk1.a@j1rek
    let prepared = !pins.is_empty() || work.prepares_bindings();
    let mut pre = pre_event_from_work(
        &command,
        current_dir.clone(),
        ctx.via,
        ctx.audience,
        event::actor(&saved_user),
        &work,
    );
    // Before the ticket: an error past its write would leave it behind.
    let patterned = config.collect_for_with_pattern(matched, ctx.audience)?;
    let id = write_ticket(ctx, &pre)?;
    // § f18xcggkd.a@mea09
    let uses_hub = work.needs_inject() || !pins.is_empty();
    let slot = if uses_hub {
        put_first_t(&id, notice)
    } else {
        None
    };
    // § end f18xcggkd.a

    // § betd87z6r.d@6jhnh
    if let Err(e) = prompt::resolve_disclaimers(ctx, &work.disclaimers, &command).await {
        if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() {
            // § t7kcndtra.c@2w15t
            // § one bvttr6qk1.b@1c6fw
            diary.write(|| {
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
                )
            });
            pre.pending = true;
            ticket::replace(&id, &pre)?;
            crate::hub::put_t(&id);
            // `lade approve` finds the pending ticket through LADE_T: the
            // cancel erases LADE, so no `lade unset` deletes it.
            println!(
                "{}",
                shell.set(HashMap::from([(crate::preexec::LADE_T.to_string(), id)]))
            );
            super::refuse_preexec(shell, crate::exit_codes::DISCLAIMER_WITHHELD);
        }
        // § gy20cgb72.c@a4gb5
        crate::hub::unlink_t(&id);
        let _ = ticket::unlink(&id);
        return Err(e);
    }
    let hydrate_started = std::time::Instant::now();
    let tunnel_cwd = current_dir.clone();
    // A pending ticket from another directory still in `LADE_T`: its cache
    // body is another rule's.
    // § nv6nfbbs2.b@pbet9
    let same_rules = work
        .cwd
        .as_ref()
        .is_none_or(|cwd| crate::pretool::same_dir(cwd, &current_dir));
    // § end nv6nfbbs2.b
    let Some(((mut env, files, _sources, _maskable, warnings, cached, private), detached)) =
        acquire_secrets_and_network(
            ctx,
            SecretHydrate {
                secrets: &work.secrets,
                progress: &work.progress,
                ticket_unlink: Some(id.as_str()),
                cache: same_rules.then_some(CacheScope {
                    cwd: &current_dir,
                    walk: config.walk_hash(),
                    saved_user: &saved_user,
                    patterned: &patterned,
                }),
            },
            work.network_bindings,
            Some(shell),
            keys,
            move |bindings, sink, cancel| {
                network::start_detached_network_session(bindings, sink, &tunnel_cwd, cancel)
            },
        )
        .await
    // § end betd87z6r.d
    else {
        // The human chose to run it without Lade: the shell gets nothing.
        // § one ntr20kvnb.b@qfebv, kk9qxrvgt@wq0ey
        return Ok(slot);
    };
    let started = std::time::Instant::now();
    let refused = show_loader_warnings(ctx, &warnings).await;
    super::log_phase("warnings", started);
    if refused {
        // § gy20cgb72.c@4pmzq
        // § one fz8r6d2ka.d@y2ktx
        let _ = remove_files(&mut files.keys());
        crate::hub::unlink_t(&id);
        let _ = ticket::unlink(&id);
        super::refuse_preexec(shell, crate::exit_codes::INTERRUPTED);
    }
    restore_default_signals();
    let finish = || -> Result<String> {
        merge_env_with_conflicts(&mut env, detached.env.clone())?;
        select_tool_env(&mut env, pins.env)?;
        let started = std::time::Instant::now();
        ticket::replace(&id, &pre)?;
        if uses_hub {
            crate::hub::put_t(&id);
        }
        super::log_phase("ticket", started);
        let mut matches = work.matches;
        // § cj4kjwcp7.b@xha6g
        event::mark_cached(&mut matches, &cached);
        // § mzce8bpjn.b@tft52
        diary.write(|| {
            work_event(
                ctx,
                &saved_user,
                current_dir.clone(),
                command.clone(),
                work.agent.clone(),
                event::logged_kind(prepared),
                matches,
                Some(scrub_values(&env, &files, &private)),
                Some(hydrate_started.elapsed().as_secs_f64() * 1000.0),
            )
        });
        super::log_phase("hydrate total", hydrate_started);
        let line = stamp_preexec(shell, env, &id, original_path)?;
        // § end mzce8bpjn.b
        // The shell gets LADE_T from this line, so `lade unset` can release.
        // § gv81k6nf4.b@tgwf0
        detached.hand_to_shell(&id)?;
        Ok(line)
        // § end gv81k6nf4.b
    };
    match finish() {
        Ok(line) => {
            println!("{line}");
            Ok(slot)
        }
        // No LADE_T reaches the shell, so `lade unset` never runs for it.
        Err(e) => {
            // § gy20cgb72.c@garfx
            // § one fz8r6d2ka.d@aqyhh
            let _ = remove_files(&mut files.keys());
            crate::hub::unlink_t(&id);
            let _ = ticket::unlink(&id);
            Err(e)
        }
    }
}

/// A new ticket for this line, in place of the shell's pending one. The
/// pending ticket's hub copy is left to the hub's expiry: readers read the
/// file, and a line with no hub work must not connect to drop it.
fn write_ticket(ctx: &InvocationContext, pre: &ticket::PreEvent) -> Result<String> {
    // § f18xcggkd.a@91yab
    let id = ticket::write(pre)?;
    if let Some(pending) = ctx.ticket_id.as_deref() {
        let _ = ticket::unlink(pending);
    }
    Ok(id)
}

fn put_first_t(id: &str, notice: bool) -> Option<VersionLedger> {
    // § jpm1fzdj4.a@b5j1w
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
    let id = write_ticket(ctx, &pre)?;
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
    // § jfxr4rmae.b@xf5c7
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
        crate::preexec::RestorePayload {
            ticket: ticket_id.to_string(),
            env: previous,
        }
        .encode()?,
    );
    let set = shell.set(env);
    let clear = shell.unset(vec!["MISE_ENV".to_string(), "MISE_SETTINGS".to_string()]);
    Ok(format!("{set};{clear}"))
}
