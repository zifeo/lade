use super::*;
use crate::providers::fake_cli;
use tempfile::tempdir;

fn path_env(dir: &tempfile::TempDir) -> HashMap<String, String> {
    HashMap::from([(
        "PATH".to_string(),
        dir.path().to_string_lossy().into_owned(),
    )])
}

#[test]
fn test_parse_version_plain() {
    assert_eq!(parse_version("2.30.0"), Some(Version::new(2, 30, 0)));
}

#[test]
fn test_parse_version_with_prefix() {
    assert_eq!(parse_version("v3.76.0"), Some(Version::new(3, 76, 0)));
}

#[test]
fn test_parse_version_vault_format() {
    assert_eq!(
        parse_version("Vault v1.15.0 ('abc')"),
        Some(Version::new(1, 15, 0))
    );
}

#[test]
fn test_parse_version_none() {
    assert_eq!(parse_version("no version here"), None);
}

#[test]
fn test_specs_have_valid_min_versions() {
    for spec in CLI_SPECS {
        assert!(
            Version::parse(spec.min_version).is_ok(),
            "{} has invalid min_version {}",
            spec.scheme,
            spec.min_version
        );
    }
}

#[test]
fn test_all_supported_schemes_includes_secret_and_network() {
    let schemes = all_supported_schemes();
    assert!(schemes.contains(&"op".to_string()));
    assert!(schemes.contains(&"vault".to_string()));
    assert!(schemes.contains(&"awssm".to_string()));
    assert!(schemes.contains(&"age".to_string()));
    assert!(schemes.contains(&"sops".to_string()));
    assert!(schemes.contains(&"kubectl".to_string()));
    assert!(schemes.contains(&"tsh".to_string()));
}

#[test]
fn test_parse_network_version_two_part() {
    assert_eq!(
        parse_network_version("OpenSSH_7.6p1"),
        Some(Version::new(7, 6, 0))
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_check_warns_on_old_version() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "op", "echo '2.0.0'");
    let warnings = check(&["op".to_string()], &path_env(&fake_bin)).await;
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].name, "1Password");
    assert_eq!(warnings[0].found, "2.0.0");
}

#[tokio::test]
#[cfg(unix)]
async fn test_check_ok_on_recent_version() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "op", "echo '2.30.0'");
    let warnings = check(&["op".to_string()], &path_env(&fake_bin)).await;
    assert!(warnings.is_empty());
}

#[tokio::test]
#[cfg(unix)]
async fn test_check_skips_missing_binary() {
    let empty_bin = tempdir().unwrap();
    let warnings = check(&["op".to_string()], &path_env(&empty_bin)).await;
    assert!(warnings.is_empty());
}

#[tokio::test]
#[cfg(unix)]
async fn test_check_ignores_unknown_scheme() {
    let warnings = check(&["unknown".to_string()], &HashMap::new()).await;
    assert!(warnings.is_empty());
}

#[tokio::test]
#[cfg(unix)]
async fn test_check_warns_on_old_network_cli() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "ssh", "echo 'OpenSSH_7.0p1' >&2");
    let warnings = check(&["ssh".to_string()], &path_env(&fake_bin)).await;
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].name, "OpenSSH");
    assert_eq!(warnings[0].found, "7.0.0");
}
