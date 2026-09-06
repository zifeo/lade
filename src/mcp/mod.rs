use std::{collections::HashMap, ffi::OsString, path::Path};

use anyhow::{Result, bail};
use log::info;
use serde_json::{Value, json};
use url::Url;

use crate::{
    args::McpCommand, config::Config, context::InvocationContext, event, inject,
    message_box::MessageBox, prompt,
};

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

pub async fn run(
    command: McpCommand,
    ctx: &InvocationContext,
    config: &Config,
    current_dir: &Path,
) -> Result<Option<i32>> {
    let target = target(&command)?;
    let stored = spawn_command(&command);
    let argv = spawn_argv(&command);
    let patterned = config.collect_for_with_pattern(&target, ctx.audience);
    let rules = patterned
        .iter()
        .map(|(path, _, rule)| (path.clone(), rule.clone()))
        .collect::<Vec<_>>();
    let saved_user = crate::config::saved_user().await?;
    let work = if patterned.is_empty() {
        None
    } else {
        Some(Config::pre_event_work(&patterned, &saved_user)?)
    };
    let disclaimers = Config::disclaimers_from_rules(&rules);
    if let Err(e) = prompt::resolve_disclaimers(ctx, &disclaimers, &target).await {
        if e.downcast_ref::<prompt::DisclaimerWithheld>().is_some()
            && let Some(work) = &work
        {
            event::emit_if(
                work.log,
                event::Emit {
                    kind: event::Kind::Denied,
                    via: ctx.via,
                    audience: ctx.audience,
                    actor: event::actor(&saved_user),
                    cwd: current_dir.to_path_buf(),
                    command: stored.clone(),
                    argv: argv.clone(),
                    hydrated: None,
                    matches: work.matches.clone(),
                    hydrate_ms: None,
                    agent: crate::agent_meta::merge(serde_json::Value::Null),
                },
            );
        }
        return Err(e);
    }
    let hydrate_started = std::time::Instant::now();
    let mut access = crate::access::acquire_attached(
        config,
        &rules,
        ctx.stderr_is_terminal && !ctx.stdin_is_terminal,
    )
    .await?;
    let hydrate_ms = Some(hydrate_started.elapsed().as_secs_f64() * 1000.0);
    match &work {
        Some(work) => event::emit_if(
            work.log,
            event::Emit {
                kind: event::logged_kind(&work.matches),
                via: ctx.via,
                audience: ctx.audience,
                actor: event::actor(&saved_user),
                cwd: current_dir.to_path_buf(),
                command: stored.clone(),
                argv: argv.clone(),
                hydrated: Some(access.public_hydrate()),
                matches: work.matches.clone(),
                hydrate_ms,
                agent: crate::agent_meta::merge(serde_json::Value::Null),
            },
        ),
        None => inject::emit_seen_if_walk_log(config, ctx, &stored, current_dir, &saved_user, argv),
    }
    for warning in &access.warnings {
        MessageBox::new().warning().line(warning).print_stderr();
    }
    let result = match command.url {
        Some(raw_url) => {
            info!("mcp started transport=http");
            run_http(raw_url, access.env.clone()).await
        }
        None => {
            info!("mcp started transport=stdio");
            let mut env = access.env.clone();
            for key in crate::shell::CHILD_UNSET {
                env.remove(key);
            }
            stdio::run_stdio(command.argv, env, current_dir.to_path_buf()).await
        }
    };
    access.cleanup()?;
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
    lade_sdk::mcp::bridge_http(
        lade_sdk::mcp::HttpBridgeConfig { url, headers },
        tokio::io::stdin(),
        tokio::io::stdout(),
    )
    .await?;
    Ok(None)
}
