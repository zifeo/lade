use crate::config::*;
use tempfile::tempdir;

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
            .unwrap()
            .as_deref(),
        Some("apm://github/destructure-command-hook")
    );
    assert!(
        config
            .command_package_uri("echo", &None, Audience::Human)
            .unwrap()
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
            .unwrap()
            .is_none()
    );
}

#[test]
fn test_command_package_a_later_dot_body_replaces() {
    let root = tempdir().unwrap();
    let sub = root.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(
        root.path().join("lade.yml"),
        "\"^ls\":\n  NOTE: apm://github/destructure-command-hook\n",
    )
    .unwrap();
    std::fs::write(sub.join("lade.yml"), ".:\n  NOTE: value\n").unwrap();
    let config = LadeFile::build(sub).unwrap();
    assert!(
        config
            .command_package_uri("ls", &None, Audience::Human)
            .unwrap()
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
    let terraform = config
        .sources_for_command("terraform plan", &None, Audience::Human)
        .unwrap();
    assert!(!terraform.iter().any(|s| s.starts_with("vault://")));
    let jq_human = config
        .sources_for_command("jq .", &None, Audience::Human)
        .unwrap();
    assert!(!jq_human.iter().any(|s| s.starts_with("op://")));
    let jq_agent = config
        .sources_for_command("jq .", &None, Audience::Agent)
        .unwrap();
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
        config
            .pins_for_command("jq .", &None, Audience::Human)
            .unwrap(),
        vec![("jq".to_string(), "mise://aqua/jqlang/jq@1.7.1".to_string())]
    );
    assert!(
        config
            .pins_for_command("terraform plan", &None, Audience::Human)
            .unwrap()
            .is_empty()
    );
    let cargo_human = config
        .pins_for_command("cargo test", &None, Audience::Human)
        .unwrap();
    assert!(!cargo_human.iter().any(|(key, _)| key == "cargo"));
    assert_eq!(
        config
            .pins_for_command("cargo test", &None, Audience::Agent)
            .unwrap(),
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
            .unwrap()
            .is_none()
    );
    assert!(
        config
            .bare_version_for("echo hi", "echo", &None, Audience::Human)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        config
            .bare_version_for("echo hi", "echo", &None, Audience::Agent)
            .unwrap()
            .as_deref(),
        Some("1.0.0")
    );
}
