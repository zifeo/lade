use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::walk::walk_up;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockSlot {
    pub name: String,
    pub version: String,
    pub backend: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LockFile {
    #[serde(default)]
    tools: HashMap<String, Vec<LockTool>>,
}

#[derive(Debug, Deserialize)]
struct LockTool {
    version: String,
    backend: Option<String>,
}

pub fn find_lock(start: &Path) -> Option<PathBuf> {
    walk_up(start, |dir| {
        let path = dir.join("mise.lock");
        path.is_file().then_some(path)
    })
}

pub fn slot_for(path: &Path, names: &[&str]) -> Option<LockSlot> {
    let parsed = parse(path)?;
    for name in names {
        if let Some(slot) = parsed.tools.get(*name).and_then(|slots| slots.first()) {
            return Some(LockSlot {
                name: (*name).to_string(),
                version: slot.version.clone(),
                backend: slot.backend.clone(),
            });
        }
    }
    None
}

fn parse(path: &Path) -> Option<LockFile> {
    let bytes = std::fs::read_to_string(path).ok()?;
    toml::from_str(&bytes).ok()
}

pub fn agrees(slot: &LockSlot, version: &str, backend_id: &str) -> bool {
    if slot.version != version {
        return false;
    }
    match slot.backend.as_deref() {
        None => true,
        Some(backend) => backend == backend_id,
    }
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
        };
        assert!(agrees(&slot, "1.7.1", "aqua:jqlang/jq"));
        assert!(!agrees(&slot, "1.6.0", "aqua:jqlang/jq"));
    }

    #[test]
    fn no_lock_is_none() {
        let dir = tempdir().unwrap();
        assert!(find_lock(dir.path()).is_none());
    }

    #[test]
    fn child_lock_hides_parent_tool() {
        let root = tempdir().unwrap();
        let child = root.path().join("apps/web");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(
            root.path().join("mise.lock"),
            "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
        )
        .unwrap();
        std::fs::write(
            child.join("mise.lock"),
            "[[tools.node]]\nversion = \"24.16.0\"\n",
        )
        .unwrap();
        let found = find_lock(&child).unwrap();
        assert_eq!(found, child.join("mise.lock"));
        assert!(slot_for(&found, &["jq", "aqua:jqlang/jq"]).is_none());
        assert!(slot_for(&found, &["node"]).is_some());
    }
}
