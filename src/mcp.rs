use std::{collections::HashMap, ffi::OsString, path::Path, process::Stdio};

use anyhow::{Result, bail};
use log::info;
use tokio::io::AsyncWriteExt;
use tokio::time::{Duration, timeout};
use url::Url;

use crate::{
    args::McpCommand, config::Config, context::InvocationContext, message_box::MessageBox, prompt,
};

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
            match ctx.via.child_stamp() {
                Some(value) => {
                    env.insert(crate::shell::LADE_VIA.to_string(), value.to_string());
                }
                None => {
                    env.remove(crate::shell::LADE_VIA);
                }
            }
            run_stdio(command.argv, env, current_dir).await
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

async fn run_stdio(
    argv: Vec<OsString>,
    env: HashMap<String, String>,
    current_dir: &Path,
) -> Result<Option<i32>> {
    let program = argv
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing MCP stdio command"))?;
    let mut child = tokio::process::Command::new(program)
        .args(&argv[1..])
        .current_dir(current_dir)
        .envs(std::env::vars())
        .env_remove(crate::shell::LADE_VIA)
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut child_stdin = child.stdin.take().expect("piped stdin");
    let mut child_stdout = child.stdout.take().expect("piped stdout");
    let mut input = tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        tokio::io::copy(&mut stdin, &mut child_stdin).await
    });
    let output = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        tokio::io::copy(&mut child_stdout, &mut stdout).await?;
        stdout.flush().await
    });
    let (status, input_finished) = tokio::select! {
        status = child.wait() => (status?, false),
        input_result = &mut input => {
            input_result??;
            let status = match timeout(Duration::from_secs(1), child.wait()).await {
                Ok(status) => status?,
                Err(_) => {
                    let _ = child.kill().await;
                    child.wait().await?
                }
            };
            (status, true)
        }
    };
    if !input_finished {
        input.abort();
        let _ = input.await;
    }
    output.await??;
    Ok((!status.success()).then_some(status.code().unwrap_or(1)))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_argv_quotes_only_when_needed() {
        assert_eq!(
            canonical_argv(&[OsString::from("npx"), OsString::from("with space")]).unwrap(),
            "npx 'with space'"
        );
    }
}
