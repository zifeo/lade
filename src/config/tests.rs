#[cfg(test)]
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
    fn test_collect_no_match() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lade.yml"), "\"specific\":\n  KEY: val\n").unwrap();
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        assert!(config.collect("other").is_empty());
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
