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
mod tests {
    use super::*;
    use crate::providers::fake_cli;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::tempdir;

    // --- Unit tests for helper functions ---

    #[test]
    fn test_strip_account_host_simple() {
        assert_eq!(
            strip_account_host(
                "op://host.1password.com/vault/item/field",
                "host.1password.com"
            ),
            "op://vault/item/field"
        );
    }

    #[test]
    fn test_strip_account_host_preserves_ampersand_unencoded() {
        // & must remain literal — inject will reject it and trigger the fallback,
        // which passes vault/item as separate CLI args (not in a URL reference).
        assert_eq!(
            strip_account_host(
                "op://host.1password.com/Product&Engineering/item/field",
                "host.1password.com"
            ),
            "op://Product&Engineering/item/field"
        );
    }

    #[test]
    fn test_strip_account_host_preserves_spaces() {
        assert_eq!(
            strip_account_host(
                "op://host.1password.com/vault/Airbyte Claryo az-02/username",
                "host.1password.com"
            ),
            "op://vault/Airbyte Claryo az-02/username"
        );
    }

    // --- Integration tests that validate what is actually sent to the op CLI ---

    #[tokio::test]
    #[cfg(unix)]
    async fn test_inject_stdin_must_contain_op_scheme() {
        // Catches regressions where op:// is stripped from the inject stdin,
        // causing op inject to silently pass through the path string as the "resolved" value.
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "op",
            r#"if [ "$1" = "inject" ]; then
    IFS= read -r stdin
    case "$stdin" in
        op://*) printf 'resolved_value' ;;
        *) printf '[ERROR] stdin must start with op://, got: %s\n' "$stdin" >&2; exit 1 ;;
    esac
fi"#,
        );
        let mut p = OnePassword::new();
        p.add("op://my.1password.com/vault/item/field".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p
            .resolve(Path::new("."), &extra, &Warnings::default())
            .await
            .unwrap();
        assert_eq!(
            result
                .get("op://my.1password.com/vault/item/field")
                .unwrap(),
            "resolved_value"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_ampersand_fallback_passes_vault_as_separate_arg() {
        // Catches regressions where & is URL-encoded (%26) or embedded in a reference URL
        // instead of being passed as a literal --vault argument to op item get.
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "op",
            r#"if [ "$1" = "inject" ]; then
    echo '[ERROR] invalid character in secret reference: &' >&2
    exit 1
elif [ "$1" = "item" ] && [ "$2" = "get" ]; then
    # Expected: op item get ITEM --vault VAULT --account ACCOUNT --fields FIELD
    # $3=item  $4=--vault  $5=vault  $6=--account  $7=account  $8=--fields  $9=field
    if [ "$5" = "Product&Engineering" ]; then
        printf 'secret_from_item_get'
    else
        printf '[ERROR] --vault arg must be literal vault name, got: %s\n' "$5" >&2
        exit 1
    fi
fi"#,
        );
        let mut p = OnePassword::new();
        p.add(
            "op://my.1password.com/Product&Engineering/Airbyte Claryo az-02/username".to_string(),
        )
        .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p
            .resolve(Path::new("."), &extra, &Warnings::default())
            .await
            .unwrap();
        assert_eq!(
            result
                .get("op://my.1password.com/Product&Engineering/Airbyte Claryo az-02/username")
                .unwrap(),
            "secret_from_item_get"
        );
    }

    #[test]
    fn test_add_valid_op_scheme() {
        let mut p = OnePassword::new();
        assert!(
            p.add("op://my.1password.com/vault_uuid/item_uuid/password".to_string())
                .is_ok()
        );
    }

    #[test]
    fn test_add_rejects_wrong_scheme() {
        let mut p = OnePassword::new();
        assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_fake_cli_single_secret() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "op", "cat > /dev/null\nprintf 'op_secret_value'");

        let mut p = OnePassword::new();
        p.add("op://my.1password.com/vault_uuid/item_uuid/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p
            .resolve(Path::new("."), &extra, &Warnings::default())
            .await
            .unwrap();
        assert_eq!(
            result
                .get("op://my.1password.com/vault_uuid/item_uuid/password")
                .unwrap(),
            "op_secret_value"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_error_in_stderr() {
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "op",
            "echo '[ERROR] authentication failed' >&2\nexit 1",
        );

        let mut p = OnePassword::new();
        p.add("op://my.1password.com/vault_uuid/item_uuid/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p
            .resolve(Path::new("."), &extra, &Warnings::default())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("1Password error"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_cli_not_found() {
        let empty_bin = tempdir().unwrap();
        let mut p = OnePassword::new();
        p.add("op://my.1password.com/vault_uuid/item_uuid/password".to_string())
            .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            empty_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p
            .resolve(Path::new("."), &extra, &Warnings::default())
            .await;
        assert!(result.is_err());
    }

    // --- FAILING TESTS (fail before the fix, pass after) ---

    #[test]
    fn test_add_rejects_plus_in_field_name() {
        let mut p = OnePassword::new();
        let result = p.add("op://my.1password.com/vault_uuid/item_uuid/field+name".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("UUID"));
    }

    #[test]
    fn test_add_rejects_plus_in_item_name() {
        let mut p = OnePassword::new();
        let result = p.add("op://my.1password.com/vault_uuid/item+name/field".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("UUID"));
    }

    #[test]
    fn test_add_accepts_uuid_path_with_section() {
        let mut p = OnePassword::new();
        assert!(
            p.add(
                "op://my.1password.com/vault_uuid/item_uuid/Section_abc123/field_uuid".to_string()
            )
            .is_ok()
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_section_and_field_uuid() {
        let fake_bin = tempdir().unwrap();
        fake_cli(&fake_bin, "op", "cat > /dev/null\nprintf 'secret_value'");

        let mut p = OnePassword::new();
        p.add(
            "op://my.1password.com/vault_uuid/item_uuid/Section_abc123def/field_uuid456"
                .to_string(),
        )
        .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p
            .resolve(Path::new("."), &extra, &Warnings::default())
            .await
            .unwrap();
        assert_eq!(
            result
                .get("op://my.1password.com/vault_uuid/item_uuid/Section_abc123def/field_uuid456")
                .unwrap(),
            "secret_value"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_reference_with_ampersand_in_vault_name() {
        // inject rejects '&' → silent fallback to op read with the full reference (including host)
        let fake_bin = tempdir().unwrap();
        fake_cli(
            &fake_bin,
            "op",
            r#"if [ "$1" = "inject" ]; then
echo '[ERROR] invalid character in secret reference: &' >&2
exit 1
elif [ "$1" = "item" ]; then
printf 'secret_from_item_get'
fi"#,
        );

        let mut p = OnePassword::new();
        p.add(
            "op://my.1password.com/Product&Engineering/Airbyte Claryo az-02/username".to_string(),
        )
        .unwrap();
        let extra = HashMap::from([(
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        )]);
        let result = p
            .resolve(Path::new("."), &extra, &Warnings::default())
            .await
            .unwrap();
        assert_eq!(
            result
                .get("op://my.1password.com/Product&Engineering/Airbyte Claryo az-02/username")
                .unwrap(),
            "secret_from_item_get"
        );
    }
}
