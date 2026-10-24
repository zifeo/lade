use std::{collections::HashMap, path::Path, process::Stdio};

use anyhow::{Result, anyhow, bail};
use futures::future::try_join_all;
use rustc_hash::FxHashMap;
use serde::de::DeserializeOwned;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use url::Url;

use crate::Hydration;

pub(crate) fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

pub(crate) fn decode_seg(raw: &str, scheme: &str) -> Result<String> {
    urlencoding::decode(raw)
        .map_err(|e| anyhow!("invalid percent-encoding in {scheme}:// URL: {e}"))
        .map(|s| s.into_owned())
}

pub(crate) fn add_url(
    urls: &mut FxHashMap<Url, String>,
    value: String,
    scheme: &str,
) -> Result<()> {
    match Url::parse(&value) {
        Ok(url) if url.scheme() == scheme => {
            urls.insert(url, value);
            Ok(())
        }
        _ => bail!("Not a {scheme} scheme"),
    }
}

/// For providers that fetch per host.
pub(crate) fn add_host_url(
    urls: &mut FxHashMap<Url, String>,
    value: String,
    scheme: &str,
) -> Result<()> {
    if Url::parse(&value).is_ok_and(|url| url.host().is_none()) {
        bail!("{scheme} URI has no host: {value}");
    }
    add_url(urls, value, scheme)
}

pub(crate) async fn run_cli(
    cmd: &[&str],
    extra_env: &HashMap<String, String>,
    name: &str,
    install_url: &str,
    cwd: Option<&Path>,
) -> Result<std::process::Output> {
    let mut c = Command::new(cmd[0]);
    c.args(&cmd[1..])
        .envs(extra_env.iter())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // bash -c sources $BASH_ENV even though it does not read .bashrc.
    if cmd[0] == "bash" {
        c.env_remove("BASH_ENV");
    }
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    c.output().await.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => anyhow!(
            "{name} CLI not found. Make sure the binary is in your PATH or install it from {install_url}."
        ),
        _ => anyhow!("{name} error: {e}"),
    })
}

pub(crate) fn search_cli(
    bin: &str,
    args: &[&str],
    extra_env: &HashMap<String, String>,
) -> Result<std::process::Output> {
    std::process::Command::new(bin)
        .args(args)
        .envs(extra_env)
        .output()
        .map_err(|e| anyhow!("{bin} error: {e}"))
}

/// Runs the list CLI and maps a failing exit to `login_msg`.
pub(crate) fn search_cli_ok(
    bin: &str,
    args: &[&str],
    extra_env: &HashMap<String, String>,
    login_msg: &str,
) -> Result<std::process::Output> {
    let output = search_cli(bin, args, extra_env)?;
    if !output.status.success() {
        bail!("{login_msg}");
    }
    Ok(output)
}

/// `run_cli` with text on stdin, for CLIs that read references from input.
pub(crate) async fn run_cli_stdin(
    cmd: &[&str],
    input: &str,
    extra_env: &HashMap<String, String>,
    name: &str,
    install_url: &str,
    cwd: Option<&Path>,
) -> Result<std::process::Output> {
    let mut c = Command::new(cmd[0]);
    c.args(&cmd[1..])
        .envs(extra_env.iter())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if cmd[0] == "bash" {
        c.env_remove("BASH_ENV");
    }
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    let mut process = c.spawn()?;
    let mut stdin = process.stdin.take().expect("Failed to open stdin");
    if let Err(e) = stdin.write_all(input.as_bytes()).await
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        bail!("{name} error: {e}");
    }
    drop(stdin);
    process
        .wait_with_output()
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => anyhow!(
                "{name} CLI not found. Make sure the binary is in your PATH or install it from {install_url}."
            ),
            _ => anyhow!("{name} error: {e}"),
        })
}

/// One CLI call per group of URIs that share a batch key, then per-URI query
/// extraction. `build_cmd` and `extract_value` get the group key.
pub(crate) async fn fetch_grouped<K, C, E>(
    groups: HashMap<K, Vec<(String, Option<String>)>>,
    extra_env: &HashMap<String, String>,
    name: &str,
    install_url: &str,
    build_cmd: C,
    extract_value: E,
) -> Result<Hydration>
where
    K: Send,
    C: Fn(&K) -> Vec<String> + Sync,
    E: Fn(&str, &K) -> Result<String> + Sync,
{
    let fetches = groups.into_iter().map(|(key, uris)| {
        let cmd = build_cmd(&key);
        let extract_value = &extract_value;
        async move {
            let cmd: Vec<&str> = cmd.iter().map(String::as_str).collect();
            let output = run_cli(&cmd, extra_env, name, install_url, None).await?;
            require_cli_ok(&output, name)?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let value = extract_value(&stdout, &key)?;
            let mut hydration = Hydration::default();
            for (raw, q) in uris {
                hydration.insert(raw, json_query_string(&value, q.as_deref())?);
            }
            Ok::<_, anyhow::Error>(hydration)
        }
    });
    Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
}

pub(crate) fn require_cli_ok(output: &std::process::Output, name: &str) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if !stderr.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    bail!("{name} error: {detail}")
}

pub(crate) fn deserialize_output<T: DeserializeOwned>(
    output: &std::process::Output,
    name: &str,
) -> Result<T> {
    serde_json::from_slice(&output.stdout).map_err(|err| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow!("{name} error: {err} (stderr: {stderr})")
    })
}

pub(crate) fn env_lookup(extra_env: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = extra_env.get(*key)
            && !value.is_empty()
        {
            return Some(value.clone());
        }
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            return Some(value);
        }
    }
    None
}

pub(crate) fn json_query_string(raw: &str, query: Option<&str>) -> Result<String> {
    let Some(query) = query.filter(|q| !q.is_empty()) else {
        return Ok(raw.to_string());
    };
    let json: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| anyhow!("JSON query {query} failed: value is not JSON ({e})"))?;
    let compiled = access_json::JSONQuery::parse(query)
        .map_err(|e| anyhow!("cannot compile query {query}: {e}"))?;
    let res = compiled
        .execute(&json)
        .map_err(|e| anyhow!("query {query} failed: {e}"))?
        .ok_or_else(|| anyhow!("no query result for {query}"))?;
    Ok(match res {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    })
}

pub(crate) fn host_with_port(url: &Url) -> String {
    match url.port() {
        Some(port) => format!("{}:{}", url.host().expect("Missing host"), port),
        None => url.host().expect("Missing host").to_string(),
    }
}
