use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::context::InvocationContext;
use crate::event::{self, Emit, Kind};
use crate::network;
use crate::prompt;
use crate::shell::Shell;
use crate::ticket;

use super::acquire::acquire_secrets_and_network;
use super::{
    SecretHydrate, apply_pins, emit_seen_if_walk_log, merge_env_with_conflicts,
    pre_event_from_work, public_hydrate, resolve_provider_work, select_tool_env,
    show_loader_warnings, ticket_ready,
};

pub async fn handle_set(
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    commands: Vec<String>,
    current_dir: PathBuf,
) -> Result<()> {
    println!(
        "{}",
        shell.unset(vec![crate::shell::LADE_RESTORE.to_string()])
    );
    let command = commands.join(" ");
    let use_ticket = ticket_ready(ctx.ticket_id.as_deref());
    let saved_user = crate::config::saved_user().await?;
    let pins = apply_pins(config, &command, &current_dir, &saved_user).await?;
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
                return Ok(());
            }
            let pre = empty_pre_event(&command, current_dir.clone(), ctx, &saved_user);
            let id = ticket::write_or_replace(ctx.ticket_id.as_deref(), &pre)?;
            println!("{}", stamp_preexec(shell, pins.env, &id)?);
            return Ok(());
        }
    };

    let mut pre = pre_event_from_work(
        &command,
        current_dir.clone(),
        ctx.via,
        ctx.audience,
        event::actor(&saved_user),
        &work,
    );
    let id = ticket::write_or_replace(ctx.ticket_id.as_deref(), &pre)?;

    if let Err(e) = prompt::resolve_disclaimers(ctx, &work.disclaimers, &command).await {
        if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some() {
            event::emit_if(
                work.log,
                Emit {
                    kind: Kind::Denied,
                    via: ctx.via,
                    audience: ctx.audience,
                    actor: event::actor(&saved_user),
                    cwd: current_dir.clone(),
                    command: command.clone(),
                    argv: None,
                    hydrated: None,
                    matches: work.matches.clone(),
                    hydrate_ms: None,
                    agent: crate::agent_meta::merge(work.agent.clone()),
                },
            );
            pre.pending = true;
            ticket::replace(&id, &pre)?;
            println!(
                "{}",
                shell.set(HashMap::from([(crate::shell::LADE_T.to_string(), id)]))
            );
            std::process::exit(crate::exit_codes::DISCLAIMER_WITHHELD);
        }
        let _ = ticket::unlink(&id);
        return Err(e);
    }
    let hydrate_started = std::time::Instant::now();
    let ((mut env, files, _sources, _maskable, warnings), detached) = acquire_secrets_and_network(
        ctx,
        SecretHydrate {
            secrets: &work.secrets,
            op_sa: work.op_sa.as_deref(),
            progress: &work.progress,
            ticket_unlink: Some(id.as_str()),
        },
        work.network_bindings,
        work.log,
        network::start_detached_network_session,
    )
    .await;
    show_loader_warnings(ctx, &warnings).await;
    merge_env_with_conflicts(&mut env, detached.env)?;
    select_tool_env(&mut env, pins.env)?;
    pre.network_pids = detached.pids;
    pre.pending = false;
    ticket::replace(&id, &pre)?;
    event::emit_if(
        work.log,
        Emit {
            kind: event::logged_kind(&work.matches),
            via: ctx.via,
            audience: ctx.audience,
            actor: event::actor(&saved_user),
            cwd: current_dir.clone(),
            command: command.clone(),
            argv: None,
            hydrated: Some(public_hydrate(&env, &files)),
            matches: work.matches,
            hydrate_ms: Some(hydrate_started.elapsed().as_secs_f64() * 1000.0),
            agent: crate::agent_meta::merge(work.agent.clone()),
        },
    );
    println!("{}", stamp_preexec(shell, env, &id)?);
    Ok(())
}

fn empty_pre_event(
    command: &str,
    cwd: PathBuf,
    ctx: &InvocationContext,
    saved_user: &Option<String>,
) -> ticket::PreEvent {
    ticket::PreEvent {
        command: command.to_string(),
        cwd,
        via: ctx.via.child_stamp().unwrap_or("unknown").to_string(),
        audience: match ctx.audience {
            crate::config::Audience::Agent => "agent",
            crate::config::Audience::Human => "human",
        }
        .to_string(),
        actor: event::actor(saved_user),
        log: false,
        disclaimers: Vec::new(),
        secrets: Vec::new(),
        network: Vec::new(),
        matches: serde_json::json!([]),
        op_sa: None,
        agent: serde_json::Value::Null,
        network_pids: Vec::new(),
        pending: false,
    }
}

fn stamp_preexec(
    shell: &Shell,
    mut env: HashMap<String, String>,
    ticket_id: &str,
) -> Result<String> {
    env.insert(crate::shell::LADE_T.to_string(), ticket_id.to_string());
    let mut previous = env
        .keys()
        .map(|key| (key.clone(), std::env::var(key).ok()))
        .collect::<HashMap<_, _>>();
    previous.insert("MISE_ENV".to_string(), std::env::var("MISE_ENV").ok());
    previous.insert(
        "MISE_SETTINGS".to_string(),
        std::env::var("MISE_SETTINGS").ok(),
    );
    env.insert(
        crate::shell::LADE_RESTORE.to_string(),
        crate::shell::RestorePayload { env: previous }.encode()?,
    );
    let set = shell.set(env);
    let clear = shell.unset(vec!["MISE_ENV".to_string(), "MISE_SETTINGS".to_string()]);
    Ok(format!("{set};{clear}"))
}
