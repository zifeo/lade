use super::*;
use std::fs;

#[test]
fn plugin_missing_when_sibling_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let lade = dir.path().join("lade");
    fs::write(&lade, b"lade").unwrap();
    assert!(plugin_missing(&lade));
    fs::write(dir.path().join(PLUGIN_BIN), b"plugin").unwrap();
    assert!(!plugin_missing(&lade));
}

#[test]
fn plugin_install_retries_when_lade_is_current() {
    let dir = tempfile::tempdir().unwrap();
    let lade = dir.path().join("lade");
    fs::write(&lade, b"lade").unwrap();
    assert!(should_install_plugin(false, Some(&lade)));
    fs::write(dir.path().join(PLUGIN_BIN), b"plugin").unwrap();
    assert!(!should_install_plugin(false, Some(&lade)));
    assert!(should_install_plugin(true, Some(&lade)));
    assert!(should_install_plugin(false, None));
}

#[test]
fn newer_semver_is_an_update() {
    assert!(update_available(&Some("0.18.0".to_string()), "0.17.1"));
    assert!(!update_available(&Some("0.17.1".to_string()), "0.17.1"));
    assert!(!update_available(&None, "0.17.1"));
}

#[test]
fn no_hub_means_nothing_known() {
    let status = VersionStatus::from_ledger("0.17.1", None);
    assert_eq!(status.latest, None);
    assert_eq!(status.last_check, None);
    assert_eq!(status.check_error, None);
    assert!(!status.update_available);
    assert_eq!(status.release_line(), None);
}

#[test]
fn ledger_drives_the_release_line() {
    let status = VersionStatus::from_ledger(
        "0.17.1",
        Some(VersionLedger {
            tried_at_ms: Some(1_760_000_000_000),
            latest: Some("0.18.0".into()),
            error: None,
        }),
    );
    assert!(status.update_available);
    assert_eq!(
        status.last_check,
        DateTime::from_timestamp_millis(1_760_000_000_000)
    );
    assert_eq!(
        status.release_line().as_deref(),
        Some("New Lade release: 0.17.1 → 0.18.0.")
    );
    let current = VersionStatus::from_ledger(
        "0.18.0",
        Some(VersionLedger {
            tried_at_ms: Some(1_760_000_000_000),
            latest: Some("0.18.0".into()),
            error: None,
        }),
    );
    assert_eq!(current.release_line(), None);
}

#[test]
fn failed_fetch_keeps_the_attempt_and_error() {
    let status = VersionStatus::from_ledger(
        "0.17.1",
        Some(VersionLedger {
            tried_at_ms: Some(1_760_000_000_000),
            latest: Some("0.18.0".into()),
            error: Some("timed out".into()),
        }),
    );
    assert!(status.last_check.is_some());
    assert_eq!(status.check_error.as_deref(), Some("timed out"));
    assert!(status.update_available);
}
