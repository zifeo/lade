use std::fs;
use std::path::Path;

use anyhow::Result;

use super::agent::Agent;
use super::paths::{ItemVerb, WriteOutcome};

pub(super) const SKILL_MD: &str = include_str!("../../../.agents/skills/lade/SKILL.md");

pub(super) fn is_lade_skill(content: &str) -> bool {
    if !content.contains("\nname: lade\n") {
        return false;
    }
    content.contains("Use Lade safely with coding agents.")
        || content.contains("Lade is also called AD, AID, or LAID.")
}

pub(super) fn skill_is_current(content: &str) -> bool {
    content == SKILL_MD
}

pub(super) fn write_skill(_agent: Agent, path: &Path) -> Result<WriteOutcome> {
    if path.is_file() {
        let existing = fs::read_to_string(path).unwrap_or_default();
        if !is_lade_skill(&existing) {
            return Ok(WriteOutcome {
                verb: ItemVerb::Unmanaged,
                path: path.to_path_buf(),
            });
        }
        if skill_is_current(&existing) {
            return Ok(WriteOutcome {
                verb: ItemVerb::Current,
                path: path.to_path_buf(),
            });
        }
        fs::write(path, SKILL_MD)?;
        return Ok(WriteOutcome {
            verb: ItemVerb::Updated,
            path: path.to_path_buf(),
        });
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, SKILL_MD)?;
    Ok(WriteOutcome {
        verb: ItemVerb::Installed,
        path: path.to_path_buf(),
    })
}
