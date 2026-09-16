use crate::config::{Audience, LadeFile, RuleWhen, parse_lade_yaml, render_lade_yaml};
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_rule_config_file_only() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yml");
    std::fs::write(
        &file_path,
        b"\"cmd\":\n  \".\": { file: \"out.yaml\" }\n  KEY: val\n",
    )
    .unwrap();
    let lade_file = LadeFile::from_path(&file_path).unwrap();
    let rule = &lade_file.commands.get("cmd").unwrap()[0];
    let config = rule.config.as_ref().unwrap();
    assert_eq!(config.file, Some(PathBuf::from("out.yaml")));
    assert!(config.onepassword_service_account.is_none());
}

#[test]
fn test_rule_config_absent() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yml");
    std::fs::write(&file_path, b"\"cmd\":\n  KEY: val\n").unwrap();
    let lade_file = LadeFile::from_path(&file_path).unwrap();
    assert!(lade_file.commands.get("cmd").unwrap()[0].config.is_none());
}

#[test]
fn test_old_format_dot_string_fails() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yml");
    std::fs::write(
        &file_path,
        b"\"cmd\":\n  \".\": \"some/path\"\n  KEY: val\n",
    )
    .unwrap();
    assert!(LadeFile::from_path(&file_path).is_err());
}

#[test]
fn test_multiple_commands_in_yaml() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yml");
    std::fs::write(
        &file_path,
        "\"cmd1\":\n  KEY1: val1\n\"cmd2\":\n  KEY2: val2\n",
    )
    .unwrap();
    let lade_file = LadeFile::from_path(&file_path).unwrap();
    assert_eq!(lade_file.commands.len(), 2);
    assert!(lade_file.commands.contains_key("cmd1"));
    assert!(lade_file.commands.contains_key("cmd2"));
}

#[test]
fn test_build_single_lade_yml() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), "\"cmd\":\n  KEY: val\n").unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(config.collect("cmd").len(), 1);
}

#[test]
fn test_build_yaml_extension_fallback() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yaml"), "\"cmd\":\n  KEY: val\n").unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(config.collect("cmd").len(), 1);
}

#[test]
fn test_build_both_extensions_errors() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        "\"cmd\":\n  KEY_YAML: yaml_val\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  KEY_YML: yml_val\n",
    )
    .unwrap();
    let err = LadeFile::build(dir.path().to_path_buf())
        .err()
        .expect("both extensions must fail")
        .to_string();
    assert!(err.contains("both lade.yaml and lade.yml"), "{err}");
}

#[test]
fn test_build_stops_at_user_home() {
    let root = tempdir().unwrap();
    let home = root.path().join("home");
    let proj = home.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(root.path().join("lade.yml"), "\"cmd\":\n  ABOVE: x\n").unwrap();
    std::fs::write(home.join("lade.yml"), "\"cmd\":\n  HOME_KEY: h\n").unwrap();
    std::fs::write(proj.join("lade.yml"), "\"cmd\":\n  PROJ: p\n").unwrap();
    temp_env::with_var("HOME", Some(home.as_os_str()), || {
        let config = LadeFile::build(proj.clone()).unwrap();
        let matches = config.collect("cmd");
        let keys: Vec<String> = matches
            .iter()
            .flat_map(|(_, rule)| rule.secrets.keys().cloned())
            .collect();
        assert!(keys.contains(&"HOME_KEY".into()), "{keys:?}");
        assert!(keys.contains(&"PROJ".into()), "{keys:?}");
        assert!(!keys.contains(&"ABOVE".into()), "{keys:?}");
    });
}

#[test]
fn test_build_nested_dirs_parent_first() {
    let parent = tempdir().unwrap();
    let child = parent.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        parent.path().join("lade.yml"),
        "\"cmd\":\n  PARENT_KEY: pval\n",
    )
    .unwrap();
    std::fs::write(child.join("lade.yml"), "\"cmd\":\n  CHILD_KEY: cval\n").unwrap();
    let config = LadeFile::build(child).unwrap();
    let matches = config.collect("cmd");
    assert_eq!(matches.len(), 2);
    assert!(matches[0].1.secrets.contains_key("PARENT_KEY"));
    assert!(matches[1].1.secrets.contains_key("CHILD_KEY"));
}

#[test]
fn test_build_no_config_empty() {
    let dir = tempdir().unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(config.collect("anything").is_empty());
}

#[test]
fn test_build_invalid_regex_error() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"[invalid regex\":\n  KEY: val\n",
    )
    .unwrap();
    assert!(LadeFile::build(dir.path().to_path_buf()).is_err());
}

#[test]
fn test_pattern_list_expands_to_two_rules() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"^git \":\n  - \".\":\n      when: human\n    SOCK: human\n  - \".\":\n      when: agent\n    SOCK: agent\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let human = config.collect_for("git status", Audience::Human);
    assert_eq!(human.len(), 1);
    assert_eq!(human[0].1.config.as_ref().unwrap().when, RuleWhen::Human);
    let agent = config.collect_for("git status", Audience::Agent);
    assert_eq!(agent.len(), 1);
    assert_eq!(agent[0].1.config.as_ref().unwrap().when, RuleWhen::Agent);
}

#[test]
fn test_empty_rule_list_fails() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yml");
    std::fs::write(&file_path, "\"cmd\": []\n").unwrap();
    assert!(LadeFile::from_path(&file_path).is_err());
}

#[test]
fn hash_key_is_a_rule_not_a_version() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yaml");
    std::fs::write(&file_path, "\"#\":\n  KEY: val\n").unwrap();
    let lade_file = LadeFile::from_path(&file_path).unwrap();
    assert!(lade_file.commands.contains_key("#"));
}

#[test]
fn version_comment_then_mapping_loads() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yaml");
    std::fs::write(&file_path, "#: >=0.1.0\n\"cmd\":\n  KEY: val\n").unwrap();
    let lade_file = LadeFile::from_path(&file_path).unwrap();
    assert!(
        lade_file.commands.get("cmd").unwrap()[0]
            .secrets
            .contains_key("KEY")
    );
}

#[test]
fn missing_version_comment_still_loads() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yaml");
    std::fs::write(&file_path, "\"cmd\":\n  KEY: val\n").unwrap();
    assert!(LadeFile::from_path(&file_path).is_ok());
}

#[test]
fn version_too_new_refuses() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yaml");
    std::fs::write(&file_path, "#: >=99.0.0\n\"cmd\":\n  KEY: val\n").unwrap();
    let err = LadeFile::from_path(&file_path).unwrap_err().to_string();
    assert!(err.contains("needs Lade >=99.0.0"), "{err}");
    assert!(err.contains("lade upgrade"), "{err}");
    assert!(err.contains(env!("CARGO_PKG_VERSION")), "{err}");
}

#[test]
fn parent_version_too_new_refuses_build() {
    let parent = tempdir().unwrap();
    let child = parent.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        parent.path().join("lade.yml"),
        "#: >=99.0.0\n\"cmd\":\n  PARENT_KEY: pval\n",
    )
    .unwrap();
    std::fs::write(child.join("lade.yml"), "\"cmd\":\n  CHILD_KEY: cval\n").unwrap();
    let err = LadeFile::build(child)
        .err()
        .expect("parent version must fail");
    let full = format!("{err:#}");
    assert!(full.contains("needs Lade >=99.0.0"), "{full}");
}

#[test]
fn empty_version_comment_fails() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yaml");
    std::fs::write(&file_path, "#:\n\"cmd\":\n  KEY: val\n").unwrap();
    let err = LadeFile::from_path(&file_path).unwrap_err().to_string();
    assert!(err.contains("version is empty"), "{err}");
}

#[test]
fn render_roundtrip_keeps_version() {
    let mapping: serde_yaml::Value = serde_yaml::from_str("\"cmd\":\n  KEY: val\n").unwrap();
    let raw = render_lade_yaml(Some(">=0.18.0"), &mapping).unwrap();
    assert!(raw.starts_with("#: >=0.18.0\n"), "{raw}");
    let (req, body) = parse_lade_yaml(&raw).unwrap();
    assert_eq!(req.as_deref(), Some(">=0.18.0"));
    assert!(
        body.as_mapping()
            .is_some_and(|m| m.keys().any(|k| k.as_str() == Some("cmd")))
    );
}

#[test]
fn empty_key_is_a_command_regex_not_a_version() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yaml");
    std::fs::write(&file_path, "\"\":\n  KEY: val\n").unwrap();
    let lade_file = LadeFile::from_path(&file_path).unwrap();
    assert!(lade_file.commands.contains_key(""));
}

#[test]
fn empty_key_version_string_is_not_a_lade_version() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yaml");
    std::fs::write(&file_path, "\"\": \">0.18,<=0.20\"\n\"cmd\":\n  KEY: val\n").unwrap();
    let err = LadeFile::from_path(&file_path).unwrap_err().to_string();
    assert!(!err.contains("needs Lade"), "{err}");
    assert!(!err.contains("version is empty"), "{err}");
}

#[test]
fn version_comment_accepts_a_compound_range() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yaml");
    std::fs::write(&file_path, "#: >0.18.0,<=0.20.0\n\"cmd\":\n  KEY: val\n").unwrap();
    let lade_file = LadeFile::from_path(&file_path).unwrap();
    assert!(
        lade_file.commands.get("cmd").unwrap()[0]
            .secrets
            .contains_key("KEY")
    );
}

#[test]
fn invalid_version_range_fails() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("lade.yaml");
    std::fs::write(&file_path, "#: not a range\n\"cmd\":\n  KEY: val\n").unwrap();
    let err = LadeFile::from_path(&file_path).unwrap_err().to_string();
    assert!(err.contains("is not a semver range"), "{err}");
}
