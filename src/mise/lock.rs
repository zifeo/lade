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
