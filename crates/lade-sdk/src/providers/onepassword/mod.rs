use std::{collections::HashMap, path::Path};

use anyhow::{Result, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use log::{debug, warn};
use rustc_hash::FxHashMap;
use url::Url;

use crate::Hydration;

use super::{Provider, Warnings, add_host_url, run_cli, run_cli_stdin, search_cli_ok};

static SEP: &str = "'Km5Ge8AbNc+QSBauOIN0jg'";

/// What `op inject` (2.39) prints when it cannot parse a reference.
const INVALID_REFERENCE: &str = "invalid secret reference";

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

    let output = run_cli(
        &[
            "op",
            "item",
            "get",
            item,
            "--vault",
            vault,
            "--account",
            account,
            "--fields",
            &field_filter,
            "--reveal",
        ],
        extra_env,
        "1Password",
        "https://1password.com/downloads/command-line/",
        None,
    )
    .await?;
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
                add_host_url(&mut self.urls, value, "op")
            }
            _ => bail!("Not an op scheme"),
        }
    }

    fn timeout(&self) -> std::time::Duration {
        super::HUMAN_PROVIDER_TIMEOUT
    }

    fn name(&self) -> &'static str {
        "1Password"
    }

    fn install_url(&self) -> &'static str {
        "https://1password.com/downloads/command-line/"
    }

    fn search(&self, extra_env: &HashMap<String, String>) -> Result<Vec<String>> {
        let output = search_cli_ok(
            "op",
            &["item", "list", "--format=json"],
            extra_env,
            self.timeout(),
            "op signin required",
        )?;
        let items: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
            .map_err(|_| anyhow::anyhow!("could not parse `op item list`"))?;
        Ok(items
            .iter()
            .filter_map(|item| {
                let title = item.get("title")?.as_str()?;
                let vault = item
                    .pointer("/vault/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Private");
                Some(format!("{vault}/{title}"))
            })
            .collect())
    }

    fn batch_unit(&self) -> &'static str {
        "account"
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
        let name = self.name();
        let install_url = self.install_url();
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
                let warnings = warnings.clone();
                async move {
                    let refs = vars.into_values().collect::<Vec<_>>();
                    if refs.is_empty() {
                        return Ok::<Hydration, anyhow::Error>(Hydration::default());
                    }

                    let input = refs
                        .iter()
                        .map(|v| strip_account_host(v, &account))
                        .collect::<Vec<_>>()
                        .join(SEP);
                    let cmd = ["op", "inject", "--account", &account];
                    debug!("Lade run: {}", cmd.join(" "));
                    debug!("stdin: {input:?}");

                    let child =
                        run_cli_stdin(&cmd, &input, extra_env, name, install_url, None).await?;

                    let output = String::from_utf8_lossy(&child.stdout).trim().replace('\n', "\\n");
                    let errors = String::from_utf8_lossy(&child.stderr);

                    debug!("stderr: {:?}", errors);

                    let inject_failed = errors.contains("[ERROR]")
                        || output.contains("[ERROR]")
                        || !child.status.success();
                    // Any other failure (locked app, missing vault) would fail
                    // `op item get` too: report op's own error, not a guess.
                    if inject_failed && !errors.contains(INVALID_REFERENCE) {
                        let detail = errors.trim();
                        if detail.is_empty() {
                            bail!("1Password error: op inject failed ({})", child.status);
                        }
                        bail!("1Password error: {detail}");
                    }
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
                            let value = read_one(&account, secret_ref, extra_env).await?;
                            hydration.insert(secret_ref.clone(), value);
                        }
                        hydration
                    };

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

    fn compose_add_uri(&self, picked: &str, extras: &HashMap<String, String>) -> Option<String> {
        let field = extras
            .get("field")
            .map(String::as_str)
            .unwrap_or("password");
        Some(format!("op://{picked}/{field}"))
    }
}

pub(super) const ADD_FIELDS: &[super::add::AddField] = &[super::add::AddField {
    key: "field",
    prompt: "Field (password): ",
    default: Some("password"),
}];

#[cfg(test)]
mod tests;
