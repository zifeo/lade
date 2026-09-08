use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::error::Error;
use super::install;
use super::project;
use super::spec::Spec;

const SIDECAR_V: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Sidecar {
    v: u32,
    tool: String,
    env: HashMap<String, String>,
}

pub fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "zifeo", "lade")
        .map(|project| project.cache_dir().join("mise-env"))
        .unwrap_or_else(|| PathBuf::from(".lade-mise-env"))
}

pub fn sidecar_path(spec: &Spec) -> PathBuf {
    cache_dir()
        .join(spec.backend_slug())
        .join(format!("{}.json", spec.version))
}

pub fn load(spec: &Spec) -> Option<HashMap<String, String>> {
    let bytes = std::fs::read(sidecar_path(spec)).ok()?;
    let parsed: Sidecar = serde_json::from_slice(&bytes).ok()?;
    if parsed.v != SIDECAR_V || !tool_matches(spec, &parsed.tool) {
        return None;
    }
    Some(parsed.env)
}

pub fn store(spec: &Spec, env: &HashMap<String, String>) -> Result<(), Error> {
    let path = sidecar_path(spec);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::env(e.to_string()))?;
    }
    let body = serde_json::to_vec(&Sidecar {
        v: SIDECAR_V,
        tool: spec.backend_id(),
        env: env.clone(),
    })
    .map_err(|e| Error::env(e.to_string()))?;
    std::fs::write(path, body).map_err(|e| Error::env(e.to_string()))
}

pub async fn refresh(
    spec: &Spec,
    installs: &Path,
    cwd: &Path,
) -> Result<HashMap<String, String>, Error> {
    let tmp = tempfile::tempdir().map_err(|e| Error::env(e.to_string()))?;
    let config = tmp.path().join("mise.toml");
    std::fs::write(&config, project::pin_only_toml(spec)).map_err(|e| Error::env(e.to_string()))?;
    let ignored = project::isolate_config_paths(cwd);
    let args = vec![
        "env".to_string(),
        "--json-extended".to_string(),
        spec.cli_spec(),
    ];
    let output =
        install::run_mise(installs, tmp.path(), args.clone(), Some(config), &ignored).await?;
    if !output.status.success() {
        return Err(Error::env(install::failure_detail(&output, &args)));
    }
    let env = take_this_tool(&output.stdout, spec)?;
    store(spec, &env)?;
    Ok(env)
}

fn take_this_tool(bytes: &[u8], spec: &Spec) -> Result<HashMap<String, String>, Error> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| Error::env(e.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::env("mise env --json-extended was not an object".to_string()))?;
    let mut env = HashMap::new();
    for (key, item) in object {
        if !keep_key(key) {
            continue;
        }
        let serde_json::Value::Object(fields) = item else {
            continue;
        };
        let Some(value) = fields.get("value").and_then(|v| v.as_str()) else {
            continue;
        };
        if fields
            .get("tool")
            .and_then(|v| v.as_str())
            .is_some_and(|tool| tool_matches(spec, tool))
        {
            env.insert(key.clone(), value.to_string());
        }
    }
    Ok(env)
}

fn keep_key(key: &str) -> bool {
    key != "PATH" && !key.starts_with("MISE_")
}

fn tool_matches(spec: &Spec, tool: &str) -> bool {
    tool == spec.backend_id()
        || tool == spec.short_name()
        || tool == spec.package
        || tool == spec.cli_spec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mise::spec::parse;
    use tempfile::tempdir;

    #[test]
    fn load_missing_is_none() {
        let dir = tempdir().unwrap();
        let spec = parse("mise://core/rust@1.96.0").unwrap();
        temp_env::with_var("HOME", Some(dir.path()), || {
            assert!(load(&spec).is_none());
        });
    }

    #[test]
    fn store_then_load_roundtrip() {
        let dir = tempdir().unwrap();
        let spec = parse("mise://core/rust@1.96.0").unwrap();
        let env = HashMap::from([("RUSTUP_TOOLCHAIN".to_string(), "1.96.0".to_string())]);
        temp_env::with_var("HOME", Some(dir.path()), || {
            store(&spec, &env).unwrap();
            assert_eq!(load(&spec).unwrap(), env);
            assert_eq!(
                sidecar_path(&spec),
                cache_dir().join("core-rust").join("1.96.0.json")
            );
        });
    }

    #[test]
    fn sidecar_paths_are_backend_and_version() {
        let dir = tempdir().unwrap();
        temp_env::with_var("HOME", Some(dir.path()), || {
            let rust = parse("mise://core/rust@1.96.0").unwrap();
            let jq = parse("mise://aqua/jqlang/jq@1.7.1").unwrap();
            assert_eq!(
                sidecar_path(&rust),
                cache_dir().join("core-rust/1.96.0.json")
            );
            assert_eq!(
                sidecar_path(&jq),
                cache_dir().join("aqua-jqlang-jq/1.7.1.json")
            );
        });
    }

    #[test]
    fn take_this_tool_drops_other_tools_and_config_env() {
        let spec = parse("mise://core/rust@1.96.0").unwrap();
        let raw = r#"{
            "PATH": {"value": "/usr/bin"},
            "MISE_YES": {"value": "1"},
            "NODE_VERSION": {"value": "24", "source": "/home/u/.config/mise/config.toml"},
            "JAVA_HOME": {"value": "/opt/java", "tool": "java"},
            "RUSTUP_TOOLCHAIN": {"value": "1.96.0", "tool": "core:rust"},
            "CARGO_HOME": {"value": "/c", "tool": "rust"}
        }"#;
        let kept = take_this_tool(raw.as_bytes(), &spec).unwrap();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
        assert_eq!(kept.get("CARGO_HOME").unwrap(), "/c");
    }

    #[test]
    fn take_this_tool_drops_flat_strings() {
        let spec = parse("mise://core/rust@1.96.0").unwrap();
        let kept = take_this_tool(
            br#"{"PATH":"/usr/bin","JAVA_HOME":"/opt/java","RUSTUP_TOOLCHAIN":"1.96.0"}"#,
            &spec,
        )
        .unwrap();
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn take_this_tool_skips_non_string_value() {
        let spec = parse("mise://core/rust@1.96.0").unwrap();
        let kept = take_this_tool(
            br#"{"RUSTUP_TOOLCHAIN":{"value":123,"tool":"core:rust"}}"#,
            &spec,
        )
        .unwrap();
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn load_rejects_legacy_flat_sidecar() {
        let dir = tempdir().unwrap();
        let spec = parse("mise://core/rust@1.96.0").unwrap();
        temp_env::with_var("HOME", Some(dir.path()), || {
            let path = sidecar_path(&spec);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                r#"{"JAVA_HOME":"/opt/java","RUSTUP_TOOLCHAIN":"1.96.0"}"#,
            )
            .unwrap();
            assert!(load(&spec).is_none());
        });
    }

    #[test]
    fn load_rejects_other_tool_sidecar() {
        let dir = tempdir().unwrap();
        let spec = parse("mise://core/rust@1.96.0").unwrap();
        temp_env::with_var("HOME", Some(dir.path()), || {
            store(
                &spec,
                &HashMap::from([("RUSTUP_TOOLCHAIN".to_string(), "1.96.0".to_string())]),
            )
            .unwrap();
            let path = sidecar_path(&spec);
            let mut parsed: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            parsed["tool"] = serde_json::Value::String("java".to_string());
            std::fs::write(&path, serde_json::to_vec(&parsed).unwrap()).unwrap();
            assert!(load(&spec).is_none());
        });
    }
}
