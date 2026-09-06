use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::{debug, warn};
use rustc_hash::FxHashMap;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use url::Url;

use crate::Hydration;

use super::{Provider, Warnings, add_url};

static SEP: &str = "'Km5Ge8AbNc+QSBauOIN0jg'";

#[derive(Default)]
pub struct OnePassword {
    urls: FxHashMap<Url, String>,
}

impl OnePassword {
    pub fn new() -> Self {
        Default::default()
    }
}

fn strip_account_host(value: &str, account: &str) -> String {
    value
        .strip_prefix("op://")
        .and_then(|s| s.strip_prefix(account))
        .and_then(|s| s.strip_prefix('/'))
        .map(|path| format!("op://{path}"))
        .unwrap_or_else(|| value.to_string())
}

/// Fallback for when `op inject` rejects a reference (e.g. '&' in vault/item names).
/// Uses `op item get` with vault/item/field as separate CLI arguments so special chars
/// never hit op's reference-URL parser.
async fn read_one(
    account: &str,
    secret_ref: &str,
    extra_env: &HashMap<String, String>,
) -> Result<String> {
    // Strip "op://host/" prefix to get "vault/item/[section/]field"
    let path = secret_ref
        .strip_prefix(&format!("op://{account}/"))
        .or_else(|| {
            secret_ref
                .strip_prefix("op://")
                .and_then(|s| s.find('/').map(|i| &s[i + 1..]))
        })
        .ok_or_else(|| anyhow::anyhow!("1Password: cannot parse reference: {secret_ref}"))?;

    let parts: Vec<&str> = path.splitn(4, '/').collect();
    let (vault, item, field_filter) = match parts.as_slice() {
        [v, i, f] => (*v, *i, format!("label={f}")),
        [v, i, _section, f] => (*v, *i, format!("label={f}")),
        _ => bail!("1Password: invalid reference (need vault/item/field): {secret_ref}"),
    };

    let process = Command::new("op")
        .args([
            "item", "get", item,
            "--vault", vault,
            "--account", account,
            "--fields", &field_filter,
            "--reveal",
        ])
        .envs(extra_env.iter())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => anyhow::anyhow!(
                "1Password CLI not found. Make sure the binary is in your PATH or install it from https://1password.com/downloads/command-line/."
            ),
            _ => anyhow::anyhow!("1Password error: {e}"),
        })?;
    let output = process.wait_with_output().await?;
    if !output.status.success() {
        bail!(
            "1Password error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .replace('\n', "\\n"))
}

#[async_trait]
impl Provider for OnePassword {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            Ok(url) if url.scheme() == "op" => {
                if url.path().contains('+') {
                    bail!(
                        "1Password secret references cannot contain '+' in any path segment. \
                         Use the item or field UUID instead (found with: op item get 'NAME' --format json)."
                    );
                }
                add_url(&mut self.urls, value, "op")
            }
            _ => bail!("Not an op scheme"),
        }
    }

    fn name(&self) -> &'static str {
        "1Password"
    }

    fn install_url(&self) -> &'static str {
        "https://1password.com/downloads/command-line/"
    }

    fn has_work(&self) -> bool {
        !self.urls.is_empty()
    }

    async fn resolve(
        &self,
        _: &Path,
        extra_env: &HashMap<String, String>,
        warnings: &Warnings,
    ) -> Result<Hydration> {
        let extra_env = Arc::new(extra_env.clone());
        let fetches = self
            .urls
            .iter()
            .into_group_map_by(|(url, _)| url.host().expect("Missing host"))
            .into_iter()
            .map(|(host, group)| {
                let vars = group
                    .into_iter()
                    .enumerate()
                    .map(|(idx, (_, value))| (idx.to_string(), value.clone()))
                    .collect::<HashMap<_, _>>();

                let account = host.to_string();
                let extra_env = Arc::clone(&extra_env);
                let warnings = warnings.clone();
                async move {
                    let refs = vars.into_values().collect::<Vec<_>>();
                    if refs.is_empty() {
                        return Ok(Hydration::default());
                    }

                    let input = refs
                        .iter()
                        .map(|v| strip_account_host(v, &account))
                        .collect::<Vec<_>>()
                        .join(SEP);
                    let cmd = &["op", "inject", "--account", &account];
                    debug!("Lade run: {}", cmd.join(" "));

                    let mut process = Command::new(cmd[0])
                        .args(&cmd[1..])
                        .envs(extra_env.iter())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .stdin(Stdio::piped())
                        .spawn()?;

                    debug!("stdin: {:?}", input);

                    let mut stdin = process.stdin.take().expect("Failed to open stdin");
                    if let Err(e) = stdin.write_all(input.as_bytes()).await
                        && e.kind() != std::io::ErrorKind::BrokenPipe
                    {
                        bail!("1Password error: {e}");
                    }
                    drop(stdin);

                    let child = match process.wait_with_output().await {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            bail!("1Password CLI not found. Make sure the binary is in your PATH or install it from https://1password.com/downloads/command-line/.")
                        },
                        Err(e) => bail!("1Password error: {e}"),
                        Ok(child) => child,
                    };

                    let output = String::from_utf8_lossy(&child.stdout).trim().replace('\n', "\\n");
                    let errors = String::from_utf8_lossy(&child.stderr);

                    debug!("stdout: {:?}", output);
                    debug!("stderr: {:?}", errors);

                    let inject_failed = errors.contains("[ERROR]")
                        || output.contains("[ERROR]")
                        || !child.status.success();
                    let loaded = output.split(SEP).collect::<Vec<_>>();

                    let hydration = if !inject_failed && loaded.len() == refs.len() {
                        refs.iter()
                            .zip_eq(loaded)
                            .map(|(key, value)| (key.clone(), value.to_string()))
                            .collect::<Hydration>()
                    } else {
                        // op inject rejects any reference whose vault/item/field name contains
                        // characters outside the allowed set (alphanumeric, -, _, ., whitespace).
                        // '&' is the most common offender. This is a known op CLI limitation with
                        // no planned fix on their side as of 2026:
                        //   https://www.1password.dev/cli/secret-reference-syntax (supported characters)
                        //   https://1password.community/discussions/developers/support-more-special-characters-in-secret-references/23363
                        // Fall back to per-secret op item get — slower but bypasses the URL parser.
                        warn!(
                            "1Password: 'op inject' failed for account {account}, falling back to per-secret 'op item get' (slower). \
                             Avoid special characters like '&' in vault/item names or use UUIDs."
                        );
                        warnings.push(format!(
                            "1Password is resolving secrets one by one for account {account} because 'op inject' rejected \
                             a reference (likely '&' in a vault or item name). This is slower. \
                             Use UUIDs or rename the vault/item to avoid special characters."
                        ));
                        let mut hydration = Hydration::default();
                        for secret_ref in &refs {
                            let value = read_one(&account, secret_ref, &extra_env).await?;
                            hydration.insert(secret_ref.clone(), value);
                        }
                        hydration
                    };

                    debug!("hydration: {:?}", hydration);
                    Ok(hydration)
                }
            })
            .collect::<Vec<_>>();

        Ok(try_join_all(fetches)
            .await?
            .into_iter()
            .flatten()
            .collect::<Hydration>())
    }
}

#[cfg(test)]
mod tests;
