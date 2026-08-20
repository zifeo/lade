#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::config::*;
    use tempfile::tempdir;

    #[test]
    fn test_collect_dot_matches_any_non_empty_command() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lade.yml"), ".:\n  KEY: val\n").unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect("git status").len(), 1);
        assert_eq!(config.collect("ssh -T git@github.com").len(), 1);
        assert!(config.collect("").is_empty());
    }

    #[test]
    fn test_collect_for_filters_when() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            ".:\n  \".\":\n    when: agent\n  SOCK: agent-sock\n\"^git \":\n  \".\":\n    when: human\n  SOCK: human-sock\n\"echo\":\n  SOCK: always-sock\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let agent = config.collect_for("git status", Audience::Agent);
        assert_eq!(agent.len(), 1);
        assert!(agent[0].1.secrets.contains_key("SOCK"));
        assert_eq!(agent[0].1.config.as_ref().unwrap().when, RuleWhen::Agent);
        let human = config.collect_for("git status", Audience::Human);
        assert_eq!(human.len(), 1);
        assert_eq!(human[0].1.config.as_ref().unwrap().when, RuleWhen::Human);
        let echo_agent = config.collect_for("echo hi", Audience::Agent);
        assert_eq!(echo_agent.len(), 2);
        let echo_human = config.collect_for("echo hi", Audience::Human);
        assert_eq!(echo_human.len(), 1);
        assert!(echo_human[0].1.config.is_none());
    }

    #[test]
    fn test_collect_for_default_when_is_always() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lade.yml"), "\"cmd\":\n  KEY: val\n").unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert_eq!(config.collect_for("cmd", Audience::Agent).len(), 1);
        assert_eq!(config.collect_for("cmd", Audience::Human).len(), 1);
    }

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

    #[tokio::test]
    async fn test_later_network_replaces_secret() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            ".:\n  DB_PORT: \"5432\"\n\"cmd\":\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let (vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
        assert!(
            vars.get(&None::<std::path::PathBuf>)
                .and_then(|env| env.get("DB_PORT"))
                .is_none()
        );
        let bindings = config.collect_network_bindings("cmd", &None);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].key, "DB_PORT");
    }

    #[tokio::test]
    async fn test_later_secret_replaces_network() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            ".:\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n\"cmd\":\n  DB_PORT: \"5432\"\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let (vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
        let env = vars.get(&None::<std::path::PathBuf>).unwrap();
        assert_eq!(env.get("DB_PORT").unwrap(), "5432");
        assert!(config.collect_network_bindings("cmd", &None).is_empty());
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
    async fn test_collect_hydrate_child_file_overlays_parent_file() {
        let parent = tempdir().unwrap();
        let child = parent.path().join("child");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(
            parent.path().join("lade.yml"),
            "\"cmd\":\n  TOKEN: parent\n",
        )
        .unwrap();
        std::fs::write(child.join("lade.yml"), "\"cmd\":\n  TOKEN: child\n").unwrap();
        let config = LadeFile::build(child).unwrap();
        let (vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
        let env = vars.get(&None::<std::path::PathBuf>).unwrap();
        assert_eq!(env.get("TOKEN").unwrap(), "child");
        let plan = config.collect_secret_sources("cmd").unwrap();
        assert!(plan.overridden.contains("TOKEN"));
        assert_eq!(plan.sources.get("TOKEN").unwrap(), "child");
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

    #[tokio::test]
    async fn test_collect_hydrate_empty_string_is_not_cancel() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            ".:\n  SSH_AUTH_SOCK: \"\"\n\"^git \":\n  SSH_AUTH_SOCK: ~\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let (vars, _, _, _) = config
            .collect_hydrate("ssh -T git@github.com")
            .await
            .unwrap();
        let env = vars.get(&None::<std::path::PathBuf>).unwrap();
        assert_eq!(env.get("SSH_AUTH_SOCK").unwrap(), "");
        let (vars, _, _, _) = config.collect_hydrate("git status").await.unwrap();
        assert!(
            vars.get(&None::<std::path::PathBuf>)
                .and_then(|env| env.get("SSH_AUTH_SOCK"))
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_collect_hydrate_later_rule_overlays_same_key() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd.*\":\n  TOKEN: parent\n\".*\":\n  TOKEN: child\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let (vars, _, _, _) = config.collect_hydrate("cmd run").await.unwrap();
        let env = vars.get(&None::<std::path::PathBuf>).unwrap();
        assert_eq!(env.get("TOKEN").unwrap(), "child");
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

    #[tokio::test]
    async fn test_collect_hydrate_null_cancels_earlier_key() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            ".:\n  TOKEN: catch\n\"^git \":\n  TOKEN: ~\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let (vars, _, _, _) = config.collect_hydrate("git status").await.unwrap();
        assert!(
            vars.get(&None::<std::path::PathBuf>)
                .and_then(|env| env.get("TOKEN"))
                .is_none()
        );
        let (vars, _, _, _) = config
            .collect_hydrate("ssh -T git@github.com")
            .await
            .unwrap();
        let env = vars.get(&None::<std::path::PathBuf>).unwrap();
        assert_eq!(env.get("TOKEN").unwrap(), "catch");
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

    #[tokio::test]
    async fn test_collect_hydrate_interpolates_private_binding_without_emitting_it() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd\":\n  .TOKEN: token\n  Authorization: \"Bearer ${TOKEN}\"\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let (vars, _, maskable, _) = config.collect_hydrate("cmd").await.unwrap();
        let env = vars.get(&None::<std::path::PathBuf>).unwrap();
        assert_eq!(env.get("Authorization"), Some(&"Bearer token".to_string()));
        assert!(!env.contains_key("TOKEN"));
        assert!(!maskable.contains("TOKEN"));
    }

    #[tokio::test]
    async fn test_collect_hydrate_rejects_public_private_collision() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            "\"cmd\":\n  TOKEN: public\n  .TOKEN: private\n",
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let err = config.collect_hydrate("cmd").await.unwrap_err();
        assert!(
            err.to_string()
                .contains("binding 'TOKEN' is declared both public and private")
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_collect_hydrate_injects_dependencies_into_shell_provider() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            r#""cmd":
  user: demo-user
  .password: demo-password
  Authorization: 'sh://printf "Basic %s" "$(printf "%s:%s" "${user}" "${.password}" | base64 | tr -d "\n")"'
"#,
        )
        .unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let (vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
        let env = vars.get(&None::<std::path::PathBuf>).unwrap();
        assert_eq!(
            env.get("Authorization"),
            Some(&"Basic ZGVtby11c2VyOmRlbW8tcGFzc3dvcmQ=".to_string())
        );
        assert_eq!(env.get("user"), Some(&"demo-user".to_string()));
        assert!(!env.contains_key("password"));
    }
}
