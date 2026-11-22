use std::{collections::HashMap, ffi::OsString, path::Path};

use anyhow::{Result, bail};
use log::info;
use serde_json::{Value, json};
use url::Url;

use crate::{args::McpCommand, config::Config, context::InvocationContext, event, prompt, wrap};

mod stdio;
#[cfg(test)]
mod tests;

/// Same bounds as `network::process`. Readiness here is not a port: an MCP
/// child is ready when spawn succeeded and the process is still running. Stdio
/// servers stay silent until `initialize`.
const INITIAL_RESTART_BACKOFF: std::time::Duration = std::time::Duration::from_millis(250);
const MAX_RESTART_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);
const STARTUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const CLIENT_EOF_GRACE: std::time::Duration = std::time::Duration::from_secs(1);
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(20);

pub async fn run(
    command: McpCommand,
    ctx: &InvocationContext,
    config: &Config,
    current_dir: &Path,
) -> Result<Option<i32>> {
    let target = target(&command)?;
    let stored = spawn_command(&command);
    let argv = spawn_argv(&command);
    let saved_user = crate::config::saved_user().await?;
    // § one g6sqmp8tb.c@2ad09
    let plan = wrap::attach::plan(ctx, config, &target, current_dir, &saved_user).await?;
    // § betd87z6r.d@9m3qa
    if let Err(e) = prompt::resolve_disclaimers(ctx, plan.disclaimers(), &target).await {
        if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some()
            && let Some((log, matches)) = plan.matched()
        {
            // § bvttr6qk1.b@m356b
            // silcrow:follow
            event::emit_if(
                log,
                event::Emit {
                    kind: event::Kind::Denied,
                    via: ctx.via,
                    audience: ctx.audience,
                    actor: event::actor(&saved_user),
                    cwd: current_dir.to_path_buf(),
                    command: stored.clone(),
                    argv: argv.clone(),
                    hydrated: None,
                    matches: matches.clone(),
                    hydrate_ms: None,
                    agent: crate::agent_meta::merge(serde_json::Value::Null),
                },
            );
        }
        // § t7kcndtra.c@84mnk
        return Err(e);
    }
    let matched = plan.matched().map(|(log, matches)| (log, matches.clone()));
    // § one bvttr6qk1.a@acaaq
    let prepared = plan.prepares();
    let hydrate_started = std::time::Instant::now();
    // § one g6sqmp8tb.c@941x3
    let access =
        wrap::attach::acquire(plan, ctx, config, &target, current_dir, &saved_user).await?;
    // § end betd87z6r.d
    let hydrate_ms = Some(hydrate_started.elapsed().as_secs_f64() * 1000.0);
    match matched {
        Some((log, mut matches)) => {
            // § cj4kjwcp7.b@cn3wz
            // silcrow:follow
            event::mark_cached(&mut matches, &access.cached);
            event::emit_if(
                log,
                event::Emit {
                    kind: event::logged_kind(prepared),
                    via: ctx.via,
                    audience: ctx.audience,
                    actor: event::actor(&saved_user),
                    cwd: current_dir.to_path_buf(),
                    command: stored.clone(),
                    argv: argv.clone(),
                    hydrated: Some(access.scrub_values()),
                    matches,
                    hydrate_ms,
                    agent: crate::agent_meta::merge(serde_json::Value::Null),
                },
            );
        }
        None => wrap::emit_seen_if_walk_log(config, ctx, &stored, current_dir, &saved_user, argv),
    }
    let result = match command.url {
        Some(raw_url) => {
            info!("mcp started transport=http");
            // The bindings become headers; a pin's PATH is no header.
            // § jfxr4rmae.a@3tdmn
            run_http(raw_url, access.env.clone()).await
        }
        None => {
            info!("mcp started transport=stdio");
            let mut env = access.child_env()?;
            // § nv6nfbbs2.c@df31s
            for key in crate::preexec::CHILD_UNSET {
                env.remove(key);
            }
            stdio::run_stdio(command.argv, env, current_dir.to_path_buf()).await
        }
    };
    drop(access);
    info!("mcp stopped");
    result
}

fn spawn_command(command: &McpCommand) -> String {
    if let Some(url) = &command.url {
        return url.clone();
    }
    command
        .argv
        .first()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn spawn_argv(command: &McpCommand) -> Option<Value> {
    if command.argv.len() < 2 {
        return None;
    }
    Some(json!(
        command.argv[1..]
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    ))
}

fn target(command: &McpCommand) -> Result<String> {
    match (&command.url, command.argv.is_empty()) {
        (Some(url), true) => Ok(url.clone()),
        (None, false) => canonical_argv(&command.argv),
        (Some(_), false) => bail!("use either an MCP URL or a stdio command after '--', not both"),
        (None, true) => bail!("provide an MCP HTTPS URL or a stdio command after '--'"),
    }
}

fn canonical_argv(argv: &[OsString]) -> Result<String> {
    argv.iter()
        .map(|value| {
            let value = value
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("MCP command arguments must be valid UTF-8"))?;
            if !value.is_empty()
                && value
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "@%+=:,./_-".contains(ch))
            {
                Ok(value.to_string())
            } else {
                Ok(format!("'{}'", value.replace('\'', "'\\''")))
            }
        })
        .collect::<Result<Vec<_>>>()
        .map(|argv| argv.join(" "))
}

/// HTTP has no local child. Transport errors are answered in-process by
/// `lade_sdk::mcp::bridge_http` and do not re-resolve secrets.
async fn run_http(raw_url: String, headers: HashMap<String, String>) -> Result<Option<i32>> {
    let url = Url::parse(&raw_url)?;
    let watch = crate::child_signals::ChildWatch::new();
    let bridge = lade_sdk::mcp::bridge_http(
        lade_sdk::mcp::HttpBridgeConfig { url, headers },
        tokio::io::stdin(),
        tokio::io::stdout(),
    );
    // A stop signal ends the bridge here, so the caller still drops the
    // access and its `.file` outputs. The exit code also keeps the runtime
    // from waiting on the stdin read.
    // § fz8r6d2ka.d@823t5
    tokio::select! {
        result = bridge => {
            result?;
            Ok(None)
        }
        () = async {
            while !watch.stop_requested() {
                tokio::time::sleep(STOP_POLL).await;
            }
        } => Ok(Some(watch.stop_code())),
    }
}
