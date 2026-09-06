use std::{collections::HashMap, ffi::OsString, path::Path};

use anyhow::{Result, bail};
use log::info;
use url::Url;

use crate::{
    args::McpCommand, config::Config, context::InvocationContext, message_box::MessageBox, prompt,
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
    let rules = config.collect_for(&target, ctx.audience);
    let disclaimers = Config::disclaimers_from_rules(&rules);
    prompt::resolve_disclaimers(ctx, &disclaimers, &target).await?;
    let mut access = crate::access::acquire_attached(
        config,
        &rules,
        ctx.stderr_is_terminal && !ctx.stdin_is_terminal,
    )
    .await?;
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
