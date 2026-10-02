use super::*;
use tempfile::tempdir;

fn entries(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, version)| ((*key).to_string(), (*version).to_string()))
        .collect()
}

#[test]
fn creates_new_file_with_only_tools() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    upsert_tools(&path, &entries(&[("jq", "1.7.1")])).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("[tools]"), "{body}");
    assert!(body.contains("jq = \"1.7.1\""), "{body}");
    assert!(!body.contains("[env]"), "{body}");
    let declared = declared_tool(&path, "jq", "aqua:jqlang/jq").unwrap();
    assert_eq!(declared.version, "1.7.1");
}

#[test]
fn preserves_env_and_comment() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(
        &path,
        "# keep this comment\n[env]\nFOO = \"bar\"\n\n[settings]\nlegacy_version_file = false\n\n[tasks.build]\nrun = \"echo hi\"\n",
    )
    .unwrap();
    upsert_tools(&path, &entries(&[("jq", "1.7.1")])).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("# keep this comment"), "{body}");
    assert!(body.contains("[env]"), "{body}");
    assert!(body.contains("FOO = \"bar\""), "{body}");
    assert!(body.contains("[settings]"), "{body}");
    assert!(body.contains("legacy_version_file = false"), "{body}");
    assert!(body.contains("[tasks.build]"), "{body}");
    assert!(body.contains("run = \"echo hi\""), "{body}");
    assert!(body.contains("jq = \"1.7.1\""), "{body}");
}

#[test]
fn string_form_overwrite() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(&path, "[tools]\njq = \"1.6.0\"\nnode = \"24\"\n").unwrap();
    upsert_tools(&path, &entries(&[("jq", "1.7.1")])).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("jq = \"1.7.1\""), "{body}");
    assert!(!body.contains("1.6.0"), "{body}");
    assert!(body.contains("node = \"24\""), "{body}");
    let declared = declared_tool(&path, "jq", "aqua:jqlang/jq").unwrap();
    assert_eq!(declared.version, "1.7.1");
}

#[test]
fn table_form_keeps_source() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(
        &path,
        "[tools]\njq = { version = \"1.6.0\", source = \"asdf\" }\n",
    )
    .unwrap();
    upsert_tools(&path, &entries(&[("jq", "1.7.1")])).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("1.7.1"), "{body}");
    assert!(!body.contains("1.6.0"), "{body}");
    assert!(body.contains("source"), "{body}");
    assert!(body.contains("asdf"), "{body}");
    let declared = declared_tool(&path, "jq", "aqua:jqlang/jq").unwrap();
    assert_eq!(declared.version, "1.7.1");
    assert_eq!(
        declared.options.get("source").map(String::as_str),
        Some("asdf")
    );
}

#[test]
fn quoted_aqua_key() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    upsert_tools(&path, &entries(&[("aqua:jqlang/jq", "1.7.1")])).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("\"aqua:jqlang/jq\""), "{body}");
    assert!(body.contains("1.7.1"), "{body}");
    let by_backend = declared_tool(&path, "jq", "aqua:jqlang/jq").unwrap();
    assert_eq!(by_backend.key, "aqua:jqlang/jq");
    assert_eq!(by_backend.version, "1.7.1");
    let by_short_name = declared_tool(&path, "jq", "github:other/jq").unwrap();
    assert_eq!(by_short_name.key, "aqua:jqlang/jq");
}

#[test]
fn empty_file_and_missing_tools_section() {
    let dir = tempdir().unwrap();
    let empty = dir.path().join("empty.toml");
    std::fs::write(&empty, "").unwrap();
    upsert_tools(&empty, &entries(&[("jq", "1.7.1")])).unwrap();
    let empty_body = std::fs::read_to_string(&empty).unwrap();
    assert!(empty_body.contains("[tools]"), "{empty_body}");
    assert!(empty_body.contains("jq = \"1.7.1\""), "{empty_body}");

    let no_tools = dir.path().join("env-only.toml");
    std::fs::write(&no_tools, "[env]\nFOO = \"bar\"\n").unwrap();
    upsert_tools(&no_tools, &entries(&[("jq", "1.7.1")])).unwrap();
    let body = std::fs::read_to_string(&no_tools).unwrap();
    assert!(body.contains("[env]"), "{body}");
    assert!(body.contains("FOO = \"bar\""), "{body}");
    assert!(body.contains("[tools]"), "{body}");
    assert!(body.contains("jq = \"1.7.1\""), "{body}");
}

#[test]
fn invalid_toml_is_error_and_file_unchanged() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    let original = "[[[not valid toml";
    std::fs::write(&path, original).unwrap();
    let err = upsert_tools(&path, &entries(&[("jq", "1.7.1")])).unwrap_err();
    assert!(err.contains("invalid TOML"), "{err}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(declared_tool(&path, "jq", "aqua:jqlang/jq").is_none());
}

#[test]
fn removes_only_named_tools() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(
        &path,
        "[env]\nFOO = \"bar\"\n\n[tools]\nnode = \"24\"\nop = \"2.30.0\"\n",
    )
    .unwrap();
    remove_tool_keys(&path, &["op".to_string()]).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("FOO = \"bar\""), "{body}");
    assert!(body.contains("node = \"24\""), "{body}");
    assert!(!body.contains("op = "), "{body}");
}

#[test]
fn declared_tool_reads_alias_string_without_options() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(&path, "[tools]\ninfisical = \"0.43.55\"\nnode = \"24\"\n").unwrap();
    let declared = declared_tool(&path, "infisical", "github:Infisical/cli").unwrap();
    assert_eq!(declared.key, "infisical");
    assert_eq!(declared.version, "0.43.55");
    assert!(declared.options.is_empty());
}

#[test]
fn declared_tool_reads_backend_key_and_its_options() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(
        &path,
        "[tools]\n\"github:Infisical/cli\" = { version = \"0.43.55\", asset_pattern = \"from-toml\" }\n",
    )
    .unwrap();
    let declared = declared_tool(&path, "infisical", "github:Infisical/cli").unwrap();
    assert_eq!(declared.key, "github:Infisical/cli");
    assert_eq!(declared.version, "0.43.55");
    assert_eq!(
        declared.options.get("asset_pattern").map(String::as_str),
        Some("from-toml")
    );
}

#[test]
fn declared_tool_matches_backend_id_when_the_alias_is_absent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(&path, "[tools]\n\"github:Infisical/cli\" = \"0.43.55\"\n").unwrap();
    let declared = declared_tool(&path, "infisical", "github:Infisical/cli").unwrap();
    assert_eq!(declared.key, "github:Infisical/cli");
    assert!(declared.options.is_empty());
}

#[test]
fn declared_tool_matches_short_name_for_the_pin_key() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(&path, "[tools]\n\"aqua:jqlang/jq\" = \"1.7.1\"\n").unwrap();
    let declared = declared_tool(&path, "jq", "aqua:jqlang/jq").unwrap();
    assert_eq!(declared.key, "aqua:jqlang/jq");
    assert_eq!(declared.version, "1.7.1");
}

#[test]
fn declared_tool_does_not_treat_cli_as_infisical() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.toml");
    std::fs::write(&path, "[tools]\n\"aqua:1password/cli\" = \"2.30.0\"\n").unwrap();
    assert!(declared_tool(&path, "infisical", "github:Infisical/cli").is_none());
}

#[test]
fn path_is_directory_is_error() {
    let dir = tempdir().unwrap();
    let err = upsert_tools(dir.path(), &entries(&[("jq", "1.7.1")])).unwrap_err();
    assert!(err.contains("directory"), "{err}");
}
