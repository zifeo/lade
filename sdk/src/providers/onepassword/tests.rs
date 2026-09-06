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
            "op://host.1password.com/Example&Team/item/field",
            "host.1password.com"
        ),
        "op://Example&Team/item/field"
    );
}

#[test]
fn test_strip_account_host_preserves_spaces() {
    assert_eq!(
        strip_account_host(
            "op://host.1password.com/vault/Example Item/username",
            "host.1password.com"
        ),
        "op://vault/Example Item/username"
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
if [ "$5" = "Example&Team" ]; then
    printf 'secret_from_item_get'
else
    printf '[ERROR] --vault arg must be literal vault name, got: %s\n' "$5" >&2
    exit 1
fi
fi"#,
    );
    let mut p = OnePassword::new();
    p.add("op://my.1password.com/Example&Team/Example Item/username".to_string())
        .unwrap();
    let extra = HashMap::from([(
        "PATH".to_string(),
        fake_bin.path().to_string_lossy().into_owned(),
    )]);
    let warnings = Warnings::default();
    let result = p.resolve(Path::new("."), &extra, &warnings).await.unwrap();
    assert_eq!(
        result
            .get("op://my.1password.com/Example&Team/Example Item/username")
            .unwrap(),
        "secret_from_item_get"
    );
    let w = warnings.take();
    assert!(!w.is_empty(), "expected a warning about op inject fallback");
    assert!(w[0].contains('&'), "warning should mention the ampersand");
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
        p.add("op://my.1password.com/vault_uuid/item_uuid/Section_abc123/field_uuid".to_string())
            .is_ok()
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_section_and_field_uuid() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "op", "cat > /dev/null\nprintf 'secret_value'");

    let mut p = OnePassword::new();
    p.add("op://my.1password.com/vault_uuid/item_uuid/Section_abc123def/field_uuid456".to_string())
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
    p.add("op://my.1password.com/Example&Team/Example Item/username".to_string())
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
            .get("op://my.1password.com/Example&Team/Example Item/username")
            .unwrap(),
        "secret_from_item_get"
    );
}
