#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::config::*;
    use tempfile::tempdir;

    #[test]
    fn test_collect_exact_match() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"terraform plan\":\n  KEY: val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect("terraform plan").len(), 1);
    }

    #[test]
    fn test_collect_regex_match() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"terraform.*\":\n  KEY: val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect("terraform plan").len(), 1);
        assert_eq!(config.collect("terraform apply").len(), 1);
        assert_eq!(config.collect("other command").len(), 0);
    }

    #[test]
    fn test_collect_disclaimers_multiple_and_deduped() {
        let dir = tempdir().unwrap();
        // Two rules match "deploy prod": one unique disclaimer each, plus a
        // duplicate shared text that must appear only once.
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"deploy\":\n  \".\":\n    disclaimer: \"Shared warning.\"\n  A: a\n\
             \"prod\":\n  \".\":\n    disclaimer: \"Shared warning.\"\n  B: b\n\
             \"deploy prod\":\n  \".\":\n    disclaimer: \"Extra warning.\"\n  C: c\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let disclaimers = config.collect_disclaimers("deploy prod");
        assert_eq!(disclaimers.len(), 2);
        assert!(disclaimers.contains(&"Shared warning.".to_string()));
        assert!(disclaimers.contains(&"Extra warning.".to_string()));
    }

    #[test]
    fn test_collect_no_match() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lade.yml"), "\"specific\":\n  KEY: val\n").unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert!(config.collect("other").is_empty());
    }

    // Shell hooks run `build` + `collect` on EVERY command, and most commands do
    // not match. This guards that common hot path against gross regressions; the
    // budget is generous (CI runners vary wildly) but still catches a 10-100x
    // slowdown. Vault resolution is intentionally excluded (it is rare and
    // network-bound).
    #[test]
    fn hot_path_build_and_no_match_is_fast() {
        use std::time::{Duration, Instant};
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"terraform .*\":\n  AWS_TOKEN: op://vault/item/field\n\"kubectl .*\":\n  KUBE_TOKEN: op://vault/item/field\n",
        )
        .unwrap();
        let path = dir.path().to_path_buf();

        for _ in 0..50 {
            let config = LadeFile::build(path.clone()).unwrap();
            assert!(config.collect("git status").is_empty());
        }

        let iters = 1000u32;
        let start = Instant::now();
        for _ in 0..iters {
            let config = LadeFile::build(path.clone()).unwrap();
            let _ = config.collect("git status --porcelain");
        }
        let per_iter = start.elapsed() / iters;

        assert!(
            per_iter < Duration::from_millis(5),
            "hot path regressed: {per_iter:?} per build+no-match (budget 5ms)"
        );
    }

    #[test]
    fn test_collect_multiple_rules_match() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd.*\":\n  KEY1: val1\n\".*\":\n  KEY2: val2\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect("cmd anything").len(), 2);
    }

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
            "\"cmd\":\n  1223: kubectl://k8s.example.com:6443/claryo-gcp-01/dev/service/postgres/5432\n  DB_PORT: kubectl://k8s.example.com:6443/claryo-gcp-01/dev/service/postgres/5432\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let bindings = config
            .collect_network_bindings("cmd", &None)
            .expect("network bindings");
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
            "\"cmd\":\n  \"1223\": kubectl://k8s.example.com:6443/claryo-gcp-01/dev/service/postgres/5432\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let bindings = config
            .collect_network_bindings("cmd", &None)
            .expect("network bindings");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].key, "1223");
    }

    #[test]
    fn test_collect_network_bindings_conflict_same_key() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd\":\n  DB_PORT: kubectl://k8s.example.com:6443/claryo-gcp-01/dev/service/postgres/5432\n\"cmd2\":\n  DB_PORT: kubectl://k8s.example.com:6443/claryo-gcp-01/dev/service/postgres/6432\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let err = config
            .collect_network_bindings("cmd cmd2", &None)
            .expect_err("conflict must fail");
        assert!(err.to_string().contains("conflicting network binding"));
    }

    #[test]
    fn test_collect_network_bindings_user_map() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd\":\n  DB_PORT:\n    alice: kubectl://a:6443/claryo-gcp-01/dev/service/postgres/5432\n    \".\": kubectl://b:6443/claryo-gcp-01/dev/service/postgres/5432\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let alice = config
            .collect_network_bindings("cmd", &Some("alice".to_string()))
            .expect("alice bindings");
        assert_eq!(
            alice[0].uri,
            "kubectl://a:6443/claryo-gcp-01/dev/service/postgres/5432"
        );
        let other = config
            .collect_network_bindings("cmd", &Some("other".to_string()))
            .expect("default bindings");
        assert_eq!(
            other[0].uri,
            "kubectl://b:6443/claryo-gcp-01/dev/service/postgres/5432"
        );
    }

    #[tokio::test]
    async fn test_collect_hydrate_rejects_numeric_non_network_key() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd\":\n  1223: plain-secret\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let err = config.collect_hydrate("cmd").await.expect_err("must fail");
        assert!(
            err.to_string()
                .contains("numeric key '1223' must use a network URI")
        );
    }

    #[test]
    fn test_collect_disclaimers() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"terraform destroy\":\n  \".\":\n    disclaimer: \"This will destroy infrastructure.\"\n  KEY: val\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let disclaimers = config.collect_disclaimers("terraform destroy");
        assert_eq!(disclaimers.len(), 1);
        assert_eq!(disclaimers[0], "This will destroy infrastructure.");
        assert!(config.collect_disclaimers("terraform plan").is_empty());
    }

    #[tokio::test]
    async fn test_collect_hydrate_fails_on_conflicting_values_for_same_output() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd.*\":\n  TOKEN: parent\n\".*\":\n  TOKEN: child\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let err = config.collect_hydrate("cmd run").await.unwrap_err();
        assert!(err.to_string().contains("conflicting value for 'TOKEN'"));
    }

    #[tokio::test]
    async fn test_collect_hydrate_allows_identical_duplicates() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd.*\":\n  TOKEN: same\n\".*\":\n  TOKEN: same\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let (vars, _, _, _) = config.collect_hydrate("cmd run").await.unwrap();
        let env = vars.get(&None::<std::path::PathBuf>).unwrap();
        assert_eq!(env.get("TOKEN").unwrap(), "same");
    }
}
