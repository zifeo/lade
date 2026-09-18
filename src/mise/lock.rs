use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::walk::walk_up;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockSlot {
    pub name: String,
    pub version: String,
    pub backend: Option<String>,
    pub checksum: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LockFile {
    #[serde(default)]
    tools: HashMap<String, Vec<LockTool>>,
}

#[derive(Debug, Deserialize)]
struct LockTool {
    version: String,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    checksum: Option<String>,
}

const LOCK_NAMES: &[&str] = &["mise.lock", "lade.lock"];

pub fn path_in(dir: &Path) -> PathBuf {
    let native = dir.join("mise.lock");
    if native.is_file() {
        native
    } else {
        dir.join("lade.lock")
    }
}

pub fn file_checksum(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let digest = Sha256::digest(bytes);
    Some(format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

#[cfg(test)]
pub fn find_lock(start: &Path) -> Option<PathBuf> {
    walk_up(start, |dir| {
        LOCK_NAMES
            .iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file())
    })
}

pub fn slot_and_path(start: &Path, names: &[&str]) -> Option<(PathBuf, LockSlot)> {
    let snap = super::plane::scan(start);
    if let Some(path) = snap.lock_path()
        && let Some(slot) = slot_for(path, names)
    {
        return Some((path.to_path_buf(), slot));
    }
    walk_up(start, |dir| {
        LOCK_NAMES.iter().find_map(|name| {
            let path = dir.join(name);
            if path.is_file() {
                slot_for(&path, names).map(|slot| (path, slot))
            } else {
                None
            }
        })
    })
}

pub fn slot_for_walk(start: &Path, names: &[&str]) -> Option<LockSlot> {
    slot_and_path(start, names).map(|(_, slot)| slot)
}

pub fn write_tools(path: &Path, slots: &[LockSlot]) -> std::io::Result<()> {
    let merge = path.file_name().is_some_and(|name| name == "mise.lock") && path.is_file();
    if merge {
        let mut all = read_tools(path).unwrap_or_default();
        for slot in slots {
            upsert(&mut all, slot.clone());
        }
        write_lock_body(path, &all)
    } else {
        write_lock_body(path, slots)
    }
}

fn write_lock_body(path: &Path, slots: &[LockSlot]) -> std::io::Result<()> {
    let mut body = String::from("lockfile_version = 1\n\n");
    for slot in slots {
        let key = if slot
            .name
            .chars()
            .any(|ch| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        {
            format!("\"{}\"", slot.name.replace('"', "\\\""))
        } else {
            slot.name.clone()
        };
        body.push_str(&format!("[[tools.{key}]]\n"));
        body.push_str(&format!(
            "version = \"{}\"\n",
            slot.version.replace('"', "\\\"")
        ));
        if let Some(backend) = &slot.backend {
            body.push_str(&format!("backend = \"{}\"\n", backend.replace('"', "\\\"")));
        }
        if let Some(checksum) = &slot.checksum {
            body.push_str(&format!(
                "checksum = \"{}\"\n",
                checksum.replace('"', "\\\"")
            ));
        }
        body.push('\n');
    }
    std::fs::write(path, body)
}

pub fn slot_for(path: &Path, names: &[&str]) -> Option<LockSlot> {
    let parsed = parse(path)?;
    for name in names {
        if let Some(slot) = parsed.tools.get(*name).and_then(|slots| slots.first()) {
            return Some(LockSlot {
                name: (*name).to_string(),
                version: slot.version.clone(),
                backend: slot.backend.clone(),
                checksum: slot.checksum.clone(),
            });
        }
    }
    None
}

fn parse(path: &Path) -> Option<LockFile> {
    let bytes = std::fs::read_to_string(path).ok()?;
    toml::from_str(&bytes).ok()
}

pub fn read_tools(path: &Path) -> Option<Vec<LockSlot>> {
    let parsed = parse(path)?;
    Some(
        parsed
            .tools
            .into_iter()
            .filter_map(|(name, tools)| {
                let slot = tools.into_iter().next()?;
                Some(LockSlot {
                    name,
                    version: slot.version,
                    backend: slot.backend,
                    checksum: slot.checksum,
                })
            })
            .collect(),
    )
}

pub fn upsert(slots: &mut Vec<LockSlot>, slot: LockSlot) {
    if let Some(existing) = slots.iter_mut().find(|s| s.name == slot.name) {
        *existing = slot;
    } else {
        slots.push(slot);
    }
}

pub fn agrees(slot: &LockSlot, version: &str, backend_id: &str) -> bool {
    let backend_ok = match slot.backend.as_deref() {
        None => true,
        Some(backend) => backend == backend_id,
    };
    if !backend_ok {
        return false;
    }
    if slot.version == version {
        return true;
    }
    if super::spec::version_is_floating(version) {
        return true;
    }
    if !super::spec::version_is_range(version) {
        return false;
    }
    let Ok(req) = semver::VersionReq::parse(version) else {
        return false;
    };
    let Ok(found) = semver::Version::parse(&slot.version) else {
        return false;
    };
    req.matches(&found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reads_array_slot() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mise.lock"),
            r#"
lockfile_version = 1

[[tools.jq]]
version = "1.7.1"
backend = "aqua:jqlang/jq"
"#,
        )
        .unwrap();
        let slot = slot_for(&dir.path().join("mise.lock"), &["jq"]).unwrap();
        assert_eq!(slot.version, "1.7.1");
        assert_eq!(slot.backend.as_deref(), Some("aqua:jqlang/jq"));
        assert!(agrees(&slot, "1.7.1", "aqua:jqlang/jq"));
        assert!(!agrees(&slot, "1.8.0", "aqua:jqlang/jq"));
        assert!(!agrees(&slot, "1.7.1", "github:jqlang/jq"));
    }

    #[test]
    fn walk_finds_parent_lock() {
        let root = tempdir().unwrap();
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(
            root.path().join("mise.lock"),
            "[[tools.jq]]\nversion = \"1.7.1\"\n",
        )
        .unwrap();
        let found = find_lock(&child).unwrap();
        assert_eq!(found, root.path().join("mise.lock"));
    }

    #[test]
    fn walk_stops_at_home() {
        let root = tempdir().unwrap();
        let home = root.path().join("home");
        let proj = home.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            root.path().join("mise.lock"),
            "[[tools.jq]]\nversion = \"1\"\n",
        )
        .unwrap();
        std::fs::write(home.join("mise.lock"), "[[tools.jq]]\nversion = \"2\"\n").unwrap();
        temp_env::with_var("HOME", Some(home.as_os_str()), || {
            let found = find_lock(&proj).unwrap();
            assert_eq!(found, home.join("mise.lock"));
        });
    }

    #[test]
    fn missing_name_is_none() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mise.lock"),
            "[[tools.jq]]\nversion = \"1.7.1\"\n",
        )
        .unwrap();
        assert!(slot_for(&dir.path().join("mise.lock"), &["node"]).is_none());
    }

    #[test]
    fn slot_for_backend_id_key() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mise.lock"),
            "[[tools.\"aqua:jqlang/jq\"]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
        )
        .unwrap();
        let slot = slot_for(
            &dir.path().join("mise.lock"),
            &["jq", "aqua:jqlang/jq", "aqua-jqlang-jq"],
        )
        .unwrap();
        assert_eq!(slot.name, "aqua:jqlang/jq");
        assert_eq!(slot.version, "1.7.1");
        assert!(agrees(&slot, "1.7.1", "aqua:jqlang/jq"));
    }

    #[test]
    fn agrees_when_lock_omits_backend() {
        let slot = LockSlot {
            name: "jq".to_string(),
            version: "1.7.1".to_string(),
            backend: None,
            checksum: None,
        };
        assert!(agrees(&slot, "1.7.1", "aqua:jqlang/jq"));
        assert!(!agrees(&slot, "1.6.0", "aqua:jqlang/jq"));
    }

    #[test]
    fn agrees_when_lock_satisfies_range() {
        let slot = LockSlot {
            name: "op".to_string(),
            version: "2.31.0".to_string(),
            backend: Some("aqua:1password/op".to_string()),
            checksum: None,
        };
        assert!(agrees(&slot, ">=2.18.0", "aqua:1password/op"));
        assert!(!agrees(&slot, ">=3.0.0", "aqua:1password/op"));
        assert!(!agrees(&slot, "2.31.0", "aqua:other/op"));
        assert!(agrees(&slot, "latest", "aqua:1password/op"));
    }

    #[test]
    fn write_tools_includes_checksum() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.lock");
        write_tools(
            &path,
            &[LockSlot {
                name: "jq".to_string(),
                version: "1.7.1".to_string(),
                backend: Some("aqua:jqlang/jq".to_string()),
                checksum: Some("sha256:abc".to_string()),
            }],
        )
        .unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("checksum = \"sha256:abc\""), "{body}");
        let slot = slot_for(&path, &["jq"]).unwrap();
        assert_eq!(slot.checksum.as_deref(), Some("sha256:abc"));
    }

    #[test]
    fn no_lock_is_none() {
        let dir = tempdir().unwrap();
        assert!(find_lock(dir.path()).is_none());
    }

    #[test]
    fn child_lock_walk_finds_parent_tool() {
        let root = tempdir().unwrap();
        let child = root.path().join("apps/web");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(
            root.path().join("lade.lock"),
            "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
        )
        .unwrap();
        std::fs::write(
            child.join("lade.lock"),
            "[[tools.node]]\nversion = \"24.16.0\"\n",
        )
        .unwrap();
        let found = find_lock(&child).unwrap();
        assert_eq!(found, child.join("lade.lock"));
        assert!(slot_for(&found, &["jq", "aqua:jqlang/jq"]).is_none());
        assert!(slot_for_walk(&child, &["jq", "aqua:jqlang/jq"]).is_some());
        assert!(slot_for_walk(&child, &["node"]).is_some());
    }

    #[test]
    fn path_in_keeps_native_mise_lock() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mise.lock"), "lockfile_version = 1\n").unwrap();
        assert_eq!(path_in(dir.path()), dir.path().join("mise.lock"));
        let empty = tempdir().unwrap();
        assert_eq!(path_in(empty.path()), empty.path().join("lade.lock"));
    }

    #[test]
    fn write_tools_merges_into_existing_mise_lock() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mise.lock");
        write_tools(
            &path,
            &[LockSlot {
                name: "node".to_string(),
                version: "24.16.0".to_string(),
                backend: None,
                checksum: None,
            }],
        )
        .unwrap();
        write_tools(
            &path,
            &[LockSlot {
                name: "jq".to_string(),
                version: "1.7.1".to_string(),
                backend: Some("aqua:jqlang/jq".to_string()),
                checksum: None,
            }],
        )
        .unwrap();
        let slots = read_tools(&path).unwrap();
        assert!(slots.iter().any(|slot| slot.name == "node"));
        assert!(
            slots
                .iter()
                .any(|slot| slot.name == "jq" && slot.version == "1.7.1")
        );
    }
}
