use super::*;
use crate::providers::fake_cli;
use std::path::Path;
use tempfile::tempdir;

const ITEMS: &str = r#"[
  {
    "id": "11111111-1111-1111-1111-111111111111",
    "name": "GitHub",
    "notes": "recovery",
    "login": {"username": "octo", "password": "p4ss", "totp": "otpauth://totp/GitHub"},
    "fields": [{"name": "api", "value": "tok"}]
  },
  {
    "id": "22222222-2222-2222-2222-222222222222",
    "name": "Dup",
    "login": {"password": "one"}
  },
  {
    "id": "33333333-3333-3333-3333-333333333333",
    "name": "Dup",
    "login": {"password": "two"}
  }
]"#;

fn path_env(dir: &tempfile::TempDir) -> HashMap<String, String> {
    HashMap::from([(
        "PATH".to_string(),
        dir.path().to_string_lossy().into_owned(),
    )])
}

#[test]
fn test_add_routing() {
    let mut p = Bitwarden::new();
    assert!(p.add("bw://GitHub/password".to_string()).is_ok());
    assert!(p.add("bw://GitHub".to_string()).is_ok());
    assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    assert_eq!(p.transport(), Transport::Cli);
    assert_eq!(p.batch_unit(), "vault");
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_fields_one_list() {
    let fake_bin = tempdir().unwrap();
    let calls = fake_bin.path().join("calls");
    fake_cli(
        &fake_bin,
        "bw",
        &format!("echo x >> '{}'; echo '{ITEMS}'", calls.display()),
    );
    let mut p = Bitwarden::new();
    p.add("bw://GitHub/password".to_string()).unwrap();
    p.add("bw://GitHub/username".to_string()).unwrap();
    p.add("bw://GitHub/notes".to_string()).unwrap();
    p.add("bw://GitHub/api".to_string()).unwrap();
    p.add("bw://11111111-1111-1111-1111-111111111111/totp".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get("bw://GitHub/password").unwrap(), "p4ss");
    assert_eq!(result.get("bw://GitHub/username").unwrap(), "octo");
    assert_eq!(result.get("bw://GitHub/notes").unwrap(), "recovery");
    assert_eq!(result.get("bw://GitHub/api").unwrap(), "tok");
    assert_eq!(
        result
            .get("bw://11111111-1111-1111-1111-111111111111/totp")
            .unwrap(),
        "otpauth://totp/GitHub"
    );
    let calls = std::fs::read_to_string(calls).unwrap();
    assert_eq!(calls.matches('x').count(), 1);
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_default_field_is_password() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "bw", &format!("echo '{ITEMS}'"));
    let mut p = Bitwarden::new();
    p.add("bw://GitHub".to_string()).unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get("bw://GitHub").unwrap(), "p4ss");
}

#[tokio::test]
#[cfg(unix)]
async fn test_duplicate_name_is_rejected() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "bw", &format!("echo '{ITEMS}'"));
    let mut p = Bitwarden::new();
    p.add("bw://Dup/password".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not unique"), "{err}");
}

#[tokio::test]
#[cfg(unix)]
async fn test_missing_item() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "bw", &format!("echo '{ITEMS}'"));
    let mut p = Bitwarden::new();
    p.add("bw://Missing/password".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"), "{err}");
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_cli_not_found() {
    let empty_bin = tempdir().unwrap();
    let mut p = Bitwarden::new();
    p.add("bw://GitHub/password".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&empty_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Bitwarden CLI not found"), "{err}");
}
