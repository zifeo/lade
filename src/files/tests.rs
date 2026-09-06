use super::*;
use crate::config::LadeFile;
use std::{collections::HashMap, path::PathBuf};
use tempfile::tempdir;

#[test]
fn test_split_env_only() {
    let hydration: HashMap<Output, HashMap<String, String>> = HashMap::from([(
        None,
        HashMap::from([("KEY".to_string(), "val".to_string())]),
    )]);
    let (env, files) = split_env_files(hydration);
    assert_eq!(env.get("KEY").unwrap(), "val");
    assert!(files.is_empty());
}

#[test]
fn test_split_files_only() {
    let path = PathBuf::from("/tmp/secrets_lade_test.json");
    let hydration: HashMap<Output, HashMap<String, String>> = HashMap::from([(
        Some(path.clone()),
        HashMap::from([("KEY".to_string(), "val".to_string())]),
    )]);
    let (env, files) = split_env_files(hydration);
    assert!(env.is_empty());
    assert_eq!(files.get(&path).unwrap().get("KEY").unwrap(), "val");
}

#[test]
fn test_split_mixed() {
    let path = PathBuf::from("/tmp/secrets_lade_mixed.json");
    let hydration: HashMap<Output, HashMap<String, String>> = HashMap::from([
        (
            None,
            HashMap::from([("ENV_KEY".to_string(), "env_val".to_string())]),
        ),
        (
            Some(path.clone()),
            HashMap::from([("FILE_KEY".to_string(), "file_val".to_string())]),
        ),
    ]);
    let (env, files) = split_env_files(hydration);
    assert_eq!(env.get("ENV_KEY").unwrap(), "env_val");
    assert_eq!(
        files.get(&path).unwrap().get("FILE_KEY").unwrap(),
        "file_val"
    );
}

#[test]
fn secret_progress_groups_omit_silent_keys() {
    let groups = secret_progress_groups(&SecretSources {
        sources: HashMap::from([
            ("QUIET".to_string(), "demo-user".to_string()),
            ("LOUD".to_string(), "demo-user".to_string()),
        ]),
        silent: ["QUIET".to_string()].into_iter().collect(),
        ..SecretSources::default()
    });
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].1, "Raw: LOUD");
}

#[test]
fn secret_progress_groups_omit_silent_cancelled_keys() {
    let groups = secret_progress_groups(&SecretSources {
        cancelled: HashMap::from([(
            "TOKEN".to_string(),
            "op://my.1password.eu/vault/item".to_string(),
        )]),
        silent: ["TOKEN".to_string()].into_iter().collect(),
        ..SecretSources::default()
    });
    assert!(groups.is_empty());
}

#[test]
fn secret_progress_groups_include_raw_values() {
    let groups = secret_progress_groups(&SecretSources {
        sources: HashMap::from([
            ("USER".to_string(), "demo-user".to_string()),
            (
                "PASSWORD".to_string(),
                "vault://vault.example.com/secret/password/value".to_string(),
            ),
        ]),
        ..SecretSources::default()
    });
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().any(|(_, display)| display == "Raw: USER"));
    assert!(
        groups
            .iter()
            .any(|(_, display)| display == "Vault vault.example.com: PASSWORD")
    );
}

#[test]
fn secret_progress_groups_mark_overrides_and_cancels() {
    let groups = secret_progress_groups(&SecretSources {
        sources: HashMap::from([("KEEP".to_string(), "child".to_string())]),
        overridden: ["KEEP".to_string()].into_iter().collect(),
        cancelled: HashMap::from([(
            "TOKEN".to_string(),
            "op://my.1password.eu/vault/item".to_string(),
        )]),
        ..SecretSources::default()
    });
    assert!(
        groups
            .iter()
            .any(|(_, display)| display.contains("KEEP (overridden)"))
    );
    assert!(groups.iter().any(|(_, display)| {
        display.contains("TOKEN (cancelled)") && display.contains("1Password")
    }));
}

#[test]
fn secret_progress_groups_label_cancelled_op_from_yaml() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  TOKEN: op://my.1password.eu/vault/item\n\"^git \":\n  TOKEN: ~\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let plan = config.collect_secret_sources("git status").unwrap();
    let groups = secret_progress_groups(&plan);
    assert!(
        groups.iter().any(|(_, display)| {
            display.contains("1Password my.1password.eu: TOKEN (cancelled)")
        }),
        "groups: {groups:?}"
    );
}

#[test]
fn test_write_files_json() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("output.json");
    let hydration = HashMap::from([(
        path.clone(),
        HashMap::from([("KEY".to_string(), "value".to_string())]),
    )]);
    let names = write_files(&hydration).unwrap();
    assert!(names.contains(&"KEY".to_string()));
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["KEY"].as_str().unwrap(), "value");
}

#[test]
fn test_write_files_yaml() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("output.yaml");
    let hydration = HashMap::from([(
        path.clone(),
        HashMap::from([("KEY".to_string(), "value".to_string())]),
    )]);
    write_files(&hydration).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("KEY") && content.contains("value"));
}

#[test]
fn test_write_files_already_exists_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("output.json");
    std::fs::write(&path, "{}").unwrap();
    let hydration = HashMap::from([(
        path.clone(),
        HashMap::from([("KEY".to_string(), "value".to_string())]),
    )]);
    assert!(write_files(&hydration).is_err());
}

#[test]
fn test_write_files_unsupported_extension_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("output.txt");
    let hydration = HashMap::from([(
        path.clone(),
        HashMap::from([("KEY".to_string(), "value".to_string())]),
    )]);
    assert!(write_files(&hydration).is_err());
}

#[test]
fn test_remove_files_existing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.json");
    std::fs::write(&path, "{}").unwrap();
    let files: HashMap<PathBuf, HashMap<String, String>> =
        HashMap::from([(path.clone(), HashMap::new())]);
    remove_files(&mut files.keys()).unwrap();
    assert!(!path.exists());
}

#[test]
fn test_remove_files_missing_is_noop() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nonexistent_lade_test.json");
    let files: HashMap<PathBuf, HashMap<String, String>> = HashMap::from([(path, HashMap::new())]);
    remove_files(&mut files.keys()).unwrap();
}
