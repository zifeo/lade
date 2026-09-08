use super::*;
use crate::mise::spec::parse;
use tempfile::tempdir;

#[test]
fn reads_string_and_table_versions() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(
        &path,
        r#"
[tools]
jq = "1.7.1"
node = { version = "24.16.0" }
"aqua:opentofu/opentofu" = "1.8.2"
"#,
    )
    .unwrap();
    let tools = project_tools(&path);
    assert!(tools.iter().any(|t| t.key == "jq" && t.version == "1.7.1"));
    assert!(
        tools
            .iter()
            .any(|t| t.key == "node" && t.version == "24.16.0")
    );
    assert!(
        tools
            .iter()
            .any(|t| t.key == "aqua:opentofu/opentofu" && t.version == "1.8.2")
    );
}

#[test]
fn conflict_on_same_tool_different_version() {
    let spec = parse("mise://aqua/jqlang/jq@1.7.1").unwrap();
    let theirs = vec![ProjectTool {
        key: "jq".to_string(),
        version: "1.6.0".to_string(),
    }];
    assert!(conflict(&spec, "jq", &theirs).is_some());
    let same = vec![ProjectTool {
        key: "jq".to_string(),
        version: "1.7.1".to_string(),
    }];
    assert!(conflict(&spec, "jq", &same).is_none());
}

#[test]
fn pin_only_toml_is_just_this_spec() {
    let spec = parse("mise://core/rust@1.96.0").unwrap();
    let body = pin_only_toml(&spec);
    assert_eq!(body, "[tools]\n\"core:rust\" = \"1.96.0\"\n");
    assert!(!body.contains("node"));
}

#[test]
fn compose_keeps_theirs_and_adds_pin() {
    let spec = parse("mise://aqua/jqlang/jq@1.7.1").unwrap();
    let theirs = vec![ProjectTool {
        key: "node".to_string(),
        version: "24.16.0".to_string(),
    }];
    let body = compose_toml(&theirs, &[("jq".to_string(), spec)]);
    assert!(body.contains("node = \"24.16.0\""));
    assert!(body.contains("\"aqua:jqlang/jq\" = \"1.7.1\""));
}

#[test]
fn isolate_includes_home_global_and_parent_config_dir() {
    let home = tempdir().unwrap();
    let cwd = tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".config/mise")).unwrap();
    std::fs::write(
        home.path().join(".config/mise/config.toml"),
        "[tools]\njava = \"21\"\n",
    )
    .unwrap();
    std::fs::write(home.path().join("mise.toml"), "[tools]\njava = \"21\"\n").unwrap();
    std::fs::write(cwd.path().join("mise.toml"), "[tools]\nnode = \"24\"\n").unwrap();
    let parent_config = home.path().join("other-mise");
    let xdg = home.path().join("xdg");
    std::fs::create_dir_all(xdg.join("mise")).unwrap();
    std::fs::write(xdg.join("mise/config.toml"), "[tools]\njava = \"21\"\n").unwrap();
    std::fs::create_dir_all(cwd.path().join(".config/mise")).unwrap();
    std::fs::write(
        cwd.path().join(".config/mise/config.toml"),
        "[tools]\ngo = \"1.24\"\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_CONFIG_DIR", Some(parent_config.as_path())),
            ("XDG_CONFIG_HOME", Some(xdg.as_path())),
        ],
        || {
            let isolated = isolate_config_paths(cwd.path());
            assert!(
                isolated.contains(&home.path().join(".config/mise")),
                "{isolated:?}"
            );
            assert!(isolated.contains(&xdg.join("mise")), "{isolated:?}");
            assert!(
                isolated.contains(&cwd.path().join(".config/mise")),
                "{isolated:?}"
            );
            assert!(
                isolated.contains(&home.path().join("mise.toml")),
                "{isolated:?}"
            );
            assert!(
                isolated.contains(&cwd.path().join("mise.toml")),
                "{isolated:?}"
            );
            assert!(isolated.contains(&parent_config), "{isolated:?}");
        },
    );
}
