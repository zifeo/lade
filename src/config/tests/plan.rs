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
