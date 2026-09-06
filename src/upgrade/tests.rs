use super::*;

#[test]
fn never_checked_is_always_due() {
    assert!(check_is_due(None, Utc::now()));
}

#[test]
fn recent_check_is_not_due() {
    assert!(!check_is_due(Some(Utc::now()), Utc::now()));
    assert!(!check_is_due(
        Some(Utc::now() - TimeDelta::try_hours(23).unwrap()),
        Utc::now()
    ));
}

#[test]
fn day_old_check_is_due() {
    assert!(check_is_due(
        Some(Utc::now() - TimeDelta::try_hours(25).unwrap()),
        Utc::now()
    ));
}

#[test]
fn version_change_is_due_even_when_check_is_recent() {
    assert!(daily_work_is_due(
        Some(Utc::now()),
        None,
        "0.18.0",
        Utc::now()
    ));
    assert!(daily_work_is_due(
        Some(Utc::now()),
        Some("0.17.1"),
        "0.18.0",
        Utc::now()
    ));
    assert!(!daily_work_is_due(
        Some(Utc::now()),
        Some("0.18.0"),
        "0.18.0",
        Utc::now()
    ));
}

#[test]
fn newer_semver_is_an_update() {
    assert!(update_available(&Some("0.18.0".to_string()), "0.17.1"));
    assert!(!update_available(&Some("0.17.1".to_string()), "0.17.1"));
    assert!(!update_available(&None, "0.17.1"));
}

fn write_not_due_config(path: &std::path::Path, latest: Option<&str>) {
    let config = GlobalConfig {
        update_check: Some(
            DateTime::parse_from_rfc3339("2099-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        ),
        latest_version: latest.map(str::to_string),
        self_version: Some(cargo_crate_version!().to_string()),
        user: None,
        cli_check: Default::default(),
    };
    std::fs::write(path, serde_json::to_string(&config).unwrap()).unwrap();
}

#[test]
fn fetch_skips_network_when_not_due() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    write_not_due_config(&path, None);
    temp_env::with_vars([("LADE_CONFIG_PATH", Some(path.to_str().unwrap()))], || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let config = GlobalConfig::load().await.unwrap();
                assert!(!daily_work_is_due(
                    config.update_check,
                    config.self_version.as_deref(),
                    cargo_crate_version!(),
                    Utc::now()
                ));
                let status = fetch_version_status().await.unwrap();
                assert_eq!(status.latest, None);
                assert!(!status.update_available);
                assert!(status.last_check.is_some());
            });
    });
}

#[test]
fn fetch_returns_cached_latest_when_not_due() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    write_not_due_config(&path, Some("0.18.0"));
    temp_env::with_vars([("LADE_CONFIG_PATH", Some(path.to_str().unwrap()))], || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let config = GlobalConfig::load().await.unwrap();
                assert!(!daily_work_is_due(
                    config.update_check,
                    config.self_version.as_deref(),
                    cargo_crate_version!(),
                    Utc::now()
                ));
                let status = fetch_version_status().await.unwrap();
                assert_eq!(status.latest.as_deref(), Some("0.18.0"));
                assert_eq!(
                    status.update_available,
                    update_available(&Some("0.18.0".to_string()), cargo_crate_version!())
                );
            });
    });
}

#[test]
fn void_daily_stamps_clears_check_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(
        &path,
        r#"{"update_check":"2099-01-01T00:00:00Z","self_version":"0.1.0","user":null,"cli_check":{}}"#,
    )
    .unwrap();
    temp_env::with_vars([("LADE_CONFIG_PATH", Some(path.to_str().unwrap()))], || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                GlobalConfig::update(void_daily_stamps).await.unwrap();
                let config = GlobalConfig::load().await.unwrap();
                assert_eq!(config.update_check, None);
                assert_eq!(config.self_version, None);
            });
    });
}

#[test]
fn stamp_daily_check_refreshes_installed_hooks() {
    let home = tempfile::tempdir().unwrap();
    let config_path = home.path().join("config.json");
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    std::fs::write(
        home.path().join(".codex").join("hooks.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lade hook"}]}]}}"#,
    )
    .unwrap();
    let home_str = home.path().to_str().unwrap();
    let config_str = config_path.to_str().unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home_str)),
            ("LADE_CONFIG_PATH", Some(config_str)),
        ],
        || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    // Same writes as `stamp_daily_check`. That helper also
                    // calls `refresh_installed`, which uses `current_dir()`
                    // (this crate) and would rewrite the repo hook files.
                    GlobalConfig::update(|c| {
                        c.update_check = Some(Utc::now());
                        c.self_version = Some("0.18.0".into());
                    })
                    .await
                    .unwrap();
                    crate::pretool::install::refresh_at(home.path(), home.path());
                    let config = GlobalConfig::load().await.unwrap();
                    assert_eq!(config.self_version.as_deref(), Some("0.18.0"));
                    assert!(config.update_check.is_some());
                });
            let body =
                std::fs::read_to_string(home.path().join(".codex").join("hooks.json")).unwrap();
            assert!(body.contains("--harness"));
        },
    );
}
