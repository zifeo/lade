use super::*;
use crate::providers::fake_cli;
use std::path::Path;
use tempfile::tempdir;

fn path_env(dir: &tempfile::TempDir) -> HashMap<String, String> {
    HashMap::from([(
        "PATH".to_string(),
        dir.path().to_string_lossy().into_owned(),
    )])
}

#[test]
fn test_add_routing() {
    let mut p = Infisical::new();
    assert!(
        p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
            .is_ok()
    );
    assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    assert!(
        p.add("infisical://app.infisical.com/proj123/dev/".to_string())
            .is_err()
    );
    assert_eq!(p.transport(), Transport::Cli);
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_single_var() {
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "infisical",
        r#"echo '[{"key":"MY_SECRET","value":"infisical_value","secretPath":"/"}]'"#,
    );
    let mut p = Infisical::new();
    p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("infisical://app.infisical.com/proj123/dev/MY_SECRET")
            .unwrap(),
        "infisical_value"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_same_path_one_call() {
    let fake_bin = tempdir().unwrap();
    let calls = fake_bin.path().join("calls");
    fake_cli(
        &fake_bin,
        "infisical",
        &format!(
            "echo x >> '{}'; echo '[{{ \"key\":\"A\",\"value\":\"one\"}},{{\"key\":\"B\",\"value\":\"two\"}}]'",
            calls.display()
        ),
    );
    let mut p = Infisical::new();
    p.add("infisical://app.infisical.com/proj123/dev/A".to_string())
        .unwrap();
    p.add("infisical://app.infisical.com/proj123/dev/B".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("infisical://app.infisical.com/proj123/dev/A")
            .unwrap(),
        "one"
    );
    assert_eq!(
        result
            .get("infisical://app.infisical.com/proj123/dev/B")
            .unwrap(),
        "two"
    );
    let calls = std::fs::read_to_string(calls).unwrap();
    assert_eq!(calls.matches('x').count(), 1);
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_percent_encoded_variable_name() {
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "infisical",
        r#"echo '[{"key":"MY+SECRET","value":"decoded_value","secretPath":"/"}]'"#,
    );
    let mut p = Infisical::new();
    p.add("infisical://app.infisical.com/proj123/dev/MY%2BSECRET".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("infisical://app.infisical.com/proj123/dev/MY%2BSECRET")
            .unwrap(),
        "decoded_value"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_nested_folder_path() {
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "infisical",
        r#"echo '[{"key":"KEY","value":"nested"}]'"#,
    );
    let mut p = Infisical::new();
    p.add("infisical://app.infisical.com/proj123/dev/My%20Folder/KEY".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("infisical://app.infisical.com/proj123/dev/My%20Folder/KEY")
            .unwrap(),
        "nested"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_missing_variable_error() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "infisical", "echo '[]'");
    let mut p = Infisical::new();
    p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await;
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_cli_not_found() {
    let empty_bin = tempdir().unwrap();
    let mut p = Infisical::new();
    p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&empty_bin), &Warnings::default())
        .await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Infisical CLI not found")
    );
}
