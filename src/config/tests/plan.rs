use crate::config::*;
use tempfile::tempdir;

#[test]
fn test_collect_keys_env_output() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  KEY1: val1\n  KEY2: val2\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let keys = config.collect_keys("cmd");
    let env_keys = keys.get(&None).unwrap();
    assert!(env_keys.contains(&"KEY1".to_string()));
    assert!(env_keys.contains(&"KEY2".to_string()));
}

#[test]
fn test_collect_keys_file_output() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  \".\": { file: \"secrets.json\" }\n  KEY: val\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let keys = config.collect_keys("cmd");
    let file_entries: Vec<_> = keys.into_iter().filter(|(k, _)| k.is_some()).collect();
    assert_eq!(file_entries.len(), 1);
    assert!(file_entries[0].1.contains(&"KEY".to_string()));
}

#[test]
fn test_collect_keys_no_match_empty() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), "\"cmd\":\n  KEY: val\n").unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(config.collect_keys("other").is_empty());
}

#[test]
fn test_collect_keys_overlays_and_null_cancel() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  KEEP: a\n  DROP: b\n\"^git \":\n  DROP: ~\n  EXTRA: c\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let keys = config.collect_keys("git status");
    let env_keys = keys.get(&None).unwrap();
    assert!(env_keys.contains(&"KEEP".to_string()));
    assert!(env_keys.contains(&"EXTRA".to_string()));
    assert!(!env_keys.contains(&"DROP".to_string()));
}

#[test]
fn test_collect_keys_for_command_uses_saved_user() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  DB_PORT:\n    alice: kubectl://a:6443/ctx/dev/service/postgres/5432\n    \".\": \"plain-default\"\n",
    )
    .unwrap();
    let home = tempdir().unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        temp_env::with_var("USER", Some("alice"), || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let keys = runtime
                .block_on(config.collect_keys_for_command("cmd"))
                .unwrap();
            let env_keys = keys.get(&None).cloned().unwrap_or_default();
            assert!(!env_keys.contains(&"DB_PORT".to_string()));
        })
    })
}

#[test]
fn test_mise_pin_is_not_a_secret() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n  SECRET: val\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let sources = config.collect_secret_sources("jq").unwrap();
    assert_eq!(sources.sources.get("SECRET").unwrap(), "val");
    assert!(!sources.sources.contains_key("jq"));
    let keys = config.collect_keys("jq");
    let env_keys = keys.get(&None).cloned().unwrap_or_default();
    assert!(env_keys.contains(&"SECRET".to_string()));
    assert!(!env_keys.contains(&"jq".to_string()));
    let pins = config.pins(&None);
    assert_eq!(
        pins,
        vec![("jq".to_string(), "mise://aqua/jqlang/jq@1.7.1".to_string())]
    );
}

#[test]
fn test_all_secret_sources_collects_values() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  KEY: plain\n  URI: op://vault/item/field\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let sources = config.all_secret_sources(&None);
    assert!(sources.contains(&"plain".to_string()));
    assert!(sources.iter().any(|s| s.starts_with("op://")));
}

#[test]
fn test_package_uris_are_not_secrets() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  guard: apm://github/destructure-command-hook\n  KEY: plain\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let sources = config.all_secret_sources(&None);
    assert!(sources.contains(&"plain".to_string()));
    assert!(!sources.iter().any(|s| s.starts_with("apm://")));
    assert_eq!(
        config.package_uris(&None),
        vec![(
            "guard".to_string(),
            "apm://github/destructure-command-hook".to_string()
        )]
    );
    assert!(config.pins(&None).is_empty());
}

#[test]
fn test_command_package_on_regex_is_flagged() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^ls:\n  NOTE: apm://github/destructure-command-hook\n.:\n  guard: apm://github/ok\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(
        config
            .command_package_uri("ls", &None, Audience::Human)
            .as_deref(),
        Some("apm://github/destructure-command-hook")
    );
    assert!(
        config
            .command_package_uri("echo", &None, Audience::Human)
            .is_none()
    );
}

#[test]
fn test_command_package_later_cancel_clears() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"^l\":\n  NOTE: apm://github/destructure-command-hook\n\"^ls\":\n  NOTE: ~\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(
        config
            .command_package_uri("ls", &None, Audience::Human)
            .is_none()
    );
}

#[test]
fn test_sources_for_command_skip_cancelled_and_when() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  TOKEN: vault://127.0.0.1:8200/secret/k/f\n\"^terraform \":\n  TOKEN: ~\n\"^jq\":\n  \".\":\n    when: agent\n  AGENT: op://v/i/f\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let terraform = config.sources_for_command("terraform plan", &None, Audience::Human);
    assert!(!terraform.iter().any(|s| s.starts_with("vault://")));
    let jq_human = config.sources_for_command("jq .", &None, Audience::Human);
    assert!(!jq_human.iter().any(|s| s.starts_with("op://")));
    let jq_agent = config.sources_for_command("jq .", &None, Audience::Agent);
    assert!(jq_agent.iter().any(|s| s.starts_with("op://")));
}

#[test]
fn test_pins_for_command_skip_cancelled_and_when() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  jq: mise://aqua/jqlang/jq@1.7.1\n\"^terraform \":\n  jq: ~\n\"^cargo\":\n  \".\":\n    when: agent\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(
        config.pins_for_command("jq .", &None, Audience::Human),
        vec![("jq".to_string(), "mise://aqua/jqlang/jq@1.7.1".to_string())]
    );
    assert!(
        config
            .pins_for_command("terraform plan", &None, Audience::Human)
            .is_empty()
    );
    let cargo_human = config.pins_for_command("cargo test", &None, Audience::Human);
    assert!(!cargo_human.iter().any(|(key, _)| key == "cargo"));
    assert_eq!(
        config.pins_for_command("cargo test", &None, Audience::Agent),
        vec![
            ("jq".to_string(), "mise://aqua/jqlang/jq@1.7.1".to_string()),
            ("cargo".to_string(), "mise://core/rust@1.96.0".to_string())
        ]
    );
}

#[test]
fn test_bare_version_for_skip_cancelled_and_when() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  jq: \"1.7.1\"\n\"^jq\":\n  jq: ~\n\"^echo\":\n  \".\":\n    when: agent\n  echo: \"1.0.0\"\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(
        config
            .bare_version_for("jq .", "jq", &None, Audience::Human)
            .is_none()
    );
    assert!(
        config
            .bare_version_for("echo hi", "echo", &None, Audience::Human)
            .is_none()
    );
    assert_eq!(
        config
            .bare_version_for("echo hi", "echo", &None, Audience::Agent)
            .as_deref(),
        Some("1.0.0")
    );
}
