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
fn test_collect_network_bindings_key_types() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  1223: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let bindings = config.collect_network_bindings("cmd", &None);
    assert_eq!(bindings.len(), 2);
    assert!(
        bindings
            .iter()
            .any(|binding| binding.key == "1223" && binding.uri.starts_with("kubectl://"))
    );
    assert!(
        bindings
            .iter()
            .any(|binding| binding.key == "DB_PORT" && binding.uri.starts_with("kubectl://"))
    );
}

#[test]
fn test_collect_network_bindings_quoted_numeric_key() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  \"1223\": kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let bindings = config.collect_network_bindings("cmd", &None);
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].key, "1223");
}

#[test]
fn test_collect_network_bindings_later_rule_overlays_same_key() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n\"cmd2\":\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/6432\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let bindings = config.collect_network_bindings("cmd cmd2", &None);
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].key, "DB_PORT");
    assert!(bindings[0].uri.ends_with("/6432"));
}

#[test]
fn test_collect_network_bindings_null_cancels_earlier_key() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n\"^git \":\n  DB_PORT: ~\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let git = config.collect_network_bindings("git status", &None);
    assert!(git.is_empty());
    let ssh = config.collect_network_bindings("ssh -T git@github.com", &None);
    assert_eq!(ssh.len(), 1);
    assert_eq!(ssh[0].key, "DB_PORT");
}

#[test]
fn test_collect_network_bindings_cancel_then_reset() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n\"cmd\":\n  DB_PORT: ~\n\"cmd run\":\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/6432\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let bindings = config.collect_network_bindings("cmd run", &None);
    assert_eq!(bindings.len(), 1);
    assert!(bindings[0].uri.ends_with("/6432"));
}

#[test]
fn test_collect_network_bindings_user_map() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  DB_PORT:\n    alice: kubectl://a:6443/example-cluster/dev/service/postgres/5432\n    \".\": kubectl://b:6443/example-cluster/dev/service/postgres/5432\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let alice = config.collect_network_bindings("cmd", &Some("alice".to_string()));
    assert_eq!(
        alice[0].uri,
        "kubectl://a:6443/example-cluster/dev/service/postgres/5432"
    );
    let other = config.collect_network_bindings("cmd", &Some("other".to_string()));
    assert_eq!(
        other[0].uri,
        "kubectl://b:6443/example-cluster/dev/service/postgres/5432"
    );
}

#[test]
fn test_secret_sources_marks_override_and_cancel() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  TOKEN: catch\n  KEEP: stay\n\"^git \":\n  TOKEN: ~\n  KEEP: git\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let git = config.collect_secret_sources("git status").unwrap();
    assert!(!git.sources.contains_key("TOKEN"));
    assert_eq!(git.cancelled.get("TOKEN").unwrap(), "catch");
    assert!(git.overridden.contains("KEEP"));
    assert_eq!(git.sources.get("KEEP").unwrap(), "git");
    let ssh = config
        .collect_secret_sources("ssh -T git@github.com")
        .unwrap();
    assert_eq!(ssh.sources.get("TOKEN").unwrap(), "catch");
    assert!(ssh.cancelled.is_empty());
    assert!(ssh.overridden.is_empty());
}

#[test]
fn test_secret_sources_cancel_then_reset() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  TOKEN: a\n\"git\":\n  TOKEN: ~\n\"git status\":\n  TOKEN: b\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let plan = config.collect_secret_sources("git status").unwrap();
    assert_eq!(plan.sources.get("TOKEN").unwrap(), "b");
    assert!(plan.cancelled.is_empty());
    assert!(plan.overridden.contains("TOKEN"));
}

#[test]
fn test_collect_secret_sources_silent_rule_marks_keys() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  \".\":\n    silence: true\n  KEY: val\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let plan = config.collect_secret_sources("cmd").unwrap();
    assert!(plan.silent.contains("KEY"));
    assert_eq!(plan.sources.get("KEY").unwrap(), "val");
}

#[test]
fn test_collect_secret_sources_later_non_silent_overlay_clears_silence() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  \".\":\n    silence: true\n  KEY: a\n\"cmd\":\n  KEY: b\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let plan = config.collect_secret_sources("cmd").unwrap();
    assert!(!plan.silent.contains("KEY"));
    assert_eq!(plan.sources.get("KEY").unwrap(), "b");
    assert!(plan.overridden.contains("KEY"));
}

#[test]
fn test_collect_secret_sources_later_silent_overlay_hides_progress() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  KEY: a\n\"cmd\":\n  \".\":\n    silence: true\n  KEY: b\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let plan = config.collect_secret_sources("cmd").unwrap();
    assert!(plan.silent.contains("KEY"));
    assert_eq!(plan.sources.get("KEY").unwrap(), "b");
}
