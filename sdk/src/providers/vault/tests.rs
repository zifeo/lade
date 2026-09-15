use super::*;
use crate::providers::{Transport, Warnings, fake_cli};
use std::path::Path;
use tempfile::tempdir;

fn path_env(dir: &tempfile::TempDir) -> HashMap<String, String> {
    HashMap::from([
        (
            "PATH".to_string(),
            dir.path().to_string_lossy().into_owned(),
        ),
        ("VAULT_TOKEN".to_string(), "s.token".to_string()),
        ("LADE_VAULT_HTTP".to_string(), "1".to_string()),
    ])
}

fn vault_json(fields: &str) -> String {
    format!(r#"echo '{{"data":{{"data":{fields}}}}}'"#)
}

#[test]
fn test_add_routing() {
    let mut p = Vault::new();
    assert!(
        p.add("vault://localhost/secret/myapp/password".to_string())
            .is_ok()
    );
    assert!(p.add("doppler://host/proj/env/VAR".to_string()).is_err());
    assert_eq!(p.transport(), Transport::Cli);
    assert_eq!(p.batch_unit(), "(host, mount, key)");
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_single_field() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "vault", &vault_json(r#"{"password":"s3cret"}"#));
    let mut p = Vault::new();
    p.add("vault://localhost/secret/myapp/password".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("vault://localhost/secret/myapp/password")
            .unwrap(),
        "s3cret"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_multiple_fields_same_key_one_call() {
    let fake_bin = tempdir().unwrap();
    let calls = fake_bin.path().join("calls");
    fake_cli(
        &fake_bin,
        "vault",
        &format!(
            "echo x >> '{}'; {}",
            calls.display(),
            vault_json(r#"{"password":"s3cret","api_key":"key123"}"#)
        ),
    );
    let mut p = Vault::new();
    p.add("vault://localhost/secret/myapp/password".to_string())
        .unwrap();
    p.add("vault://localhost/secret/myapp/api_key".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("vault://localhost/secret/myapp/password")
            .unwrap(),
        "s3cret"
    );
    assert_eq!(
        result
            .get("vault://localhost/secret/myapp/api_key")
            .unwrap(),
        "key123"
    );
    let calls = std::fs::read_to_string(fake_bin.path().join("calls")).unwrap();
    assert_eq!(calls.matches('x').count(), 1);
}

#[tokio::test]
async fn test_resolve_missing_token() {
    let home = tempfile::tempdir().unwrap();
    let mut p = Vault::new();
    p.add("vault://localhost/secret/myapp/password".to_string())
        .unwrap();
    let extra = HashMap::from([("HOME".to_string(), home.path().display().to_string())]);
    let err = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("VAULT_TOKEN"), "{err}");
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_token_file() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".vault-token"), "file-token\n").unwrap();
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "vault",
        r#"
if [ "$VAULT_TOKEN" != "file-token" ]; then
  echo "bad token $VAULT_TOKEN" >&2
  exit 1
fi
echo '{"data":{"data":{"password":"from-file"}}}'
"#,
    );
    let mut p = Vault::new();
    p.add("vault://localhost/secret/myapp/password".to_string())
        .unwrap();
    let extra = HashMap::from([
        ("HOME".to_string(), home.path().display().to_string()),
        (
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        ),
        ("LADE_VAULT_HTTP".to_string(), "1".to_string()),
    ]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("vault://localhost/secret/myapp/password")
            .unwrap(),
        "from-file"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_lade_vault_token_and_namespace() {
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "vault",
        r#"
if [ "$VAULT_TOKEN" != "lade-token" ] || [ "$VAULT_NAMESPACE" != "ns1" ]; then
  echo "bad env $VAULT_TOKEN $VAULT_NAMESPACE" >&2
  exit 1
fi
echo '{"data":{"data":{"password":"from-lade"}}}'
"#,
    );
    let mut p = Vault::new();
    p.add("vault://localhost/secret/myapp/password".to_string())
        .unwrap();
    let extra = HashMap::from([
        (
            "PATH".to_string(),
            fake_bin.path().to_string_lossy().into_owned(),
        ),
        ("LADE_VAULT_TOKEN".to_string(), "lade-token".to_string()),
        ("LADE_VAULT_NAMESPACE".to_string(), "ns1".to_string()),
        ("LADE_VAULT_HTTP".to_string(), "1".to_string()),
    ]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("vault://localhost/secret/myapp/password")
            .unwrap(),
        "from-lade"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_missing_field_names_field() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "vault", &vault_json(r#"{"other":"x"}"#));
    let mut p = Vault::new();
    p.add("vault://localhost/secret/myapp/password".to_string())
        .unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("password"), "{err}");
    assert!(!err.to_string().contains("myapp"), "{err}");
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_http_error() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "vault", "echo permission denied >&2; exit 1");
    let mut p = Vault::new();
    p.add("vault://localhost/secret/myapp/password".to_string())
        .unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Vault error"), "{err}");
}
