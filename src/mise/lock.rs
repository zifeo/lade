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

pub fn is_generated(path: &Path) -> bool {
    let Ok(body) = std::fs::read_to_string(path) else {
        return false;
    };
    body.contains("url = ") || body.contains("@generated") || body.contains("lockfile_version = 2")
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

#[cfg(test)]
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
