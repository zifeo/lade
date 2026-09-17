use anyhow::{Context, Result, bail};
use serde_yaml::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::LadeFile;
use crate::message_box::MessageBox;

use super::ask;

pub fn nearest_yaml() -> Result<Option<PathBuf>> {
    Ok(
        crate::config::yaml_files_on_walk(&std::env::current_dir()?)?
            .into_iter()
            .next(),
    )
}

pub fn target_yaml(tty: bool) -> Result<PathBuf> {
    let found = crate::config::yaml_files_on_walk(&std::env::current_dir()?)?;
    match found.as_slice() {
        [] => Ok(std::env::current_dir()?.join("lade.yaml")),
        [one] => Ok(one.clone()),
        many if tty => {
            let mut mb = MessageBox::new().info().line("lade.yaml on the walk");
            for (i, path) in many.iter().enumerate() {
                mb = mb.line(format!("  {}. {}", i + 1, path.display()));
            }
            mb.print_stderr();
            let answer = ask("Which (number, 1 is nearest): ")?;
            let index: usize = answer.parse().context("pick a number from the list")?;
            many.get(index.saturating_sub(1))
                .cloned()
                .context("pick a number from the list")
        }
        many => Ok(many[0].clone()),
    }
}

pub fn upsert_binding(path: &Path, rule: &str, key: &str, uri: &str) -> Result<()> {
    let mut file = load_file(path)?;
    {
        let map = file
            .root
            .as_mapping_mut()
            .context("lade.yaml root must be a mapping")?;
        let rule_key = Value::String(rule.to_string());
        let entry = map
            .entry(rule_key)
            .or_insert_with(|| Value::Mapping(serde_yaml::Mapping::new()));
        let rule_map = match entry {
            Value::Mapping(m) => m,
            Value::Sequence(seq) => {
                if seq.is_empty() {
                    *entry = Value::Mapping(serde_yaml::Mapping::new());
                    entry.as_mapping_mut().expect("just set mapping")
                } else {
                    bail!("rule `{rule}` is a list. Edit lade.yaml by hand.");
                }
            }
            _ => bail!("rule `{rule}` is not a mapping"),
        };
        rule_map.insert(
            Value::String(key.to_string()),
            Value::String(uri.to_string()),
        );
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    save_file(path, &file)?;
    let _ = LadeFile::from_path(path)?;
    Ok(())
}

pub fn drop_binding(path: &Path, rule: &str, key: &str) -> Result<Option<String>> {
    let mut file = load_file(path)?;
    let removed = {
        let Some(map) = file.root.as_mapping_mut() else {
            bail!("lade.yaml root must be a mapping");
        };
        let rule_key = Value::String(rule.to_string());
        let Some(entry) = map.get_mut(&rule_key) else {
            return Ok(None);
        };
        let Some(rule_map) = entry.as_mapping_mut() else {
            bail!("rule `{rule}` is not a mapping");
        };
        let removed = match rule_map.remove(Value::String(key.to_string())) {
            Some(Value::String(uri)) => Some(uri),
            Some(_) => Some(String::new()),
            None => None,
        };
        if rule_map.is_empty() {
            map.remove(&rule_key);
        }
        removed
    };
    save_file(path, &file)?;
    Ok(removed)
}

pub fn replace_binding_uri(path: &Path, key: &str, uri: &str) -> Result<bool> {
    let mut file = load_file(path)?;
    let found = {
        let Some(map) = file.root.as_mapping_mut() else {
            bail!("lade.yaml root must be a mapping");
        };
        let mut found = false;
        for (_, entry) in map.iter_mut() {
            let Some(rule_map) = entry.as_mapping_mut() else {
                continue;
            };
            let rule_key = Value::String(key.to_string());
            if rule_map.contains_key(&rule_key) {
                rule_map.insert(rule_key, Value::String(uri.to_string()));
                found = true;
            }
        }
        found
    };
    if found {
        save_file(path, &file)?;
    }
    Ok(found)
}

struct YamlFile {
    req: Option<String>,
    root: Value,
}

fn load_file(path: &Path) -> Result<YamlFile> {
    if !path.exists() {
        return Ok(YamlFile {
            req: None,
            root: Value::Mapping(serde_yaml::Mapping::new()),
        });
    }
    let raw = fs::read_to_string(path)?;
    let (req, mapping) = crate::config::parse_lade_yaml(&raw)?;
    if let Some(req_str) = req.as_deref() {
        crate::config::require_lade_version(req_str, path)?;
    }
    if !mapping.is_mapping() {
        bail!("{} is not a mapping", path.display());
    }
    Ok(YamlFile { req, root: mapping })
}

fn save_file(path: &Path, file: &YamlFile) -> Result<()> {
    fs::write(
        path,
        crate::config::render_lade_yaml(file.req.as_deref(), &file.root)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn upsert_then_remove_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        upsert_binding(&path, "^terraform", "TF_VAR_FOO", "op://v/i/f").unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("TF_VAR_FOO"));
        assert!(body.contains("op://v/i/f"));
        assert!(
            drop_binding(&path, "^terraform", "TF_VAR_FOO")
                .unwrap()
                .is_some()
        );
        let body = fs::read_to_string(&path).unwrap();
        assert!(!body.contains("TF_VAR_FOO"));
    }

    #[test]
    fn both_extensions_error() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lade.yaml"), ".\n  x: y\n").unwrap();
        fs::write(dir.path().join("lade.yml"), ".\n  x: y\n").unwrap();
        let err = crate::config::config_in_dir(dir.path()).unwrap_err();
        assert!(err.to_string().contains("both lade.yaml and lade.yml"));
    }

    #[test]
    fn replace_binding_uri_updates_the_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        upsert_binding(&path, "^jq", "jq", "mise://aqua/jqlang/jq@latest").unwrap();
        assert!(replace_binding_uri(&path, "jq", "mise://aqua/jqlang/jq@1.7.1").unwrap());
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("1.7.1"), "{body}");
        assert!(!body.contains("@latest"), "{body}");
    }

    #[test]
    fn upsert_keeps_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        fs::write(
            &path,
            ": >=0.1.0\n\"^jq\":\n  jq: mise://aqua/jqlang/jq@1.7.0\n",
        )
        .unwrap();
        upsert_binding(&path, "^jq", "jq", "mise://aqua/jqlang/jq@1.7.1").unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.starts_with(": >=0.1.0\n"), "{body}");
        assert!(!body.contains("---"), "{body}");
        assert!(body.contains("1.7.1"), "{body}");
    }
}
