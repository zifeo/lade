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
    assert!(should_install_plugin(false, &lade));
    fs::write(dir.path().join(PLUGIN_BIN), b"plugin").unwrap();
    assert!(!should_install_plugin(false, &lade));
    assert!(should_install_plugin(true, &lade));
}

#[test]
fn plugin_installs_beside_lade_from_the_release_lade_runs() {
    use self_update::update::UpdateConfig;
    let dir = tempfile::tempdir().unwrap();
    let lade = dir.path().join("lade");
    let update = configure_plugin(&lade, true, "1.2.3").unwrap();
    assert_eq!(update.bin_install_path(), dir.path().join(PLUGIN_BIN));
    assert_ne!(update.bin_install_path(), lade);
    assert_eq!(update.release_tag(), Some("v1.2.3"));
    assert_eq!(update.bin_name(), PLUGIN_BIN);
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

const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

#[test]
fn expected_checksum_reads_the_sha256_file() {
    let asset = "lade-v1.2.3-x86_64-unknown-linux-musl.tar.gz";
    let line = format!("{HELLO_SHA256}  {asset}\n");
    assert_eq!(expected_checksum(&line, asset).unwrap(), HELLO_SHA256);
    assert_eq!(
        expected_checksum(&format!("{HELLO_SHA256}\n"), asset).unwrap(),
        HELLO_SHA256
    );
    assert!(expected_checksum(&format!("{HELLO_SHA256}  other.tar.gz"), asset).is_err());
    assert!(expected_checksum("deadbeef", asset).is_err());
}

#[test]
fn a_mismatched_archive_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir
        .path()
        .join("lade-v1.2.3-x86_64-unknown-linux-musl.tar.gz");
    fs::write(&archive, b"hello").unwrap();
    verify_sha256(&archive, HELLO_SHA256).unwrap();
    let wrong = "0".repeat(64);
    let err = verify_sha256(&archive, &wrong).unwrap_err().to_string();
    assert!(err.contains("checksum mismatch"), "{err}");
    assert_eq!(fs::read(&archive).unwrap(), b"hello");
}

#[test]
fn a_release_without_a_checksum_installs_with_a_warning_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let target = "x86_64-unknown-linux-musl";
    let asset = format!("lade-v1.2.3-{target}.tar.gz");
    let archive = dir.path().join(&asset);
    fs::write(&archive, b"hello").unwrap();
    let mut asked = None;
    let warning = check_release_archive(&archive, target, |url| {
        asked = Some(url.to_string());
        Ok(None)
    })
    .unwrap()
    .expect("a warning");
    assert_eq!(
        asked.as_deref(),
        Some(
            format!("https://github.com/zifeo/lade/releases/download/v1.2.3/{asset}.sha256")
                .as_str()
        )
    );
    assert!(
        warning.contains(&format!("{asset}.sha256 is missing")),
        "{warning}"
    );
    assert!(warning.contains("unverified"), "{warning}");

    let verified = check_release_archive(&archive, target, |_| {
        Ok(Some(format!("{HELLO_SHA256}  {asset}\n")))
    });
    assert_eq!(verified.unwrap(), None);
    let refused = check_release_archive(&archive, target, |_| Ok(Some("0".repeat(64))));
    assert!(refused.is_err());
}

#[test]
fn asset_name_matches_the_release_build() {
    let target = self_update::get_target();
    let installer = include_str!("../../../installer.sh");
    assert!(
        installer.contains(&format!("PLATFORM='{target}'")),
        "{target} is not a target the installer picks"
    );
    assert!(installer.contains(r#"ASSET="$NAME-v$VERSION-$PLATFORM""#));
    assert!(installer.contains(r#"DOWNLOAD_URL="$RELEASE_URL/download/v$VERSION/$ASSET.$EXT""#));
    let asset = format!("lade-v1.2.3-{target}.tar.gz");
    assert_eq!(
        checksum_url(&asset, target).unwrap(),
        format!("https://github.com/zifeo/lade/releases/download/v1.2.3/{asset}.sha256")
    );
    assert!(checksum_url(&format!("{asset}.sha256"), target).is_err());
}

#[tokio::test]
async fn the_reload_hint_goes_out_before_a_failing_mise_step() {
    let printed = std::cell::Cell::new(false);
    let result = after_update(|| printed.set(true), async { anyhow::bail!("no mise") }).await;
    assert!(printed.get());
    assert!(result.is_err());
}
