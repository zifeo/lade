use std::fs;
use std::path::{Path, PathBuf};

use super::agent::{AGENTS, Agent};
use super::paths::{ItemVerb, short_path};
use super::ui::PretoolRow;

pub(super) const WHY_NO_SKILL: &str = "hooks wrap the command. there is no skill";

pub(super) fn is_lade_skill(content: &str) -> bool {
    if !content.contains("\nname: lade\n") {
        return false;
    }
    content.contains("Use Lade safely with coding agents.")
        || content.contains("Lade is also called AD, AID, or LAID.")
}

pub(super) fn skill_path(agent: Agent, home: &Path) -> PathBuf {
    agent
        .home_dir(home)
        .join("skills")
        .join("lade")
        .join("SKILL.md")
}

pub(super) fn project_skill_file(agent: Agent, dest: &Path) -> PathBuf {
    match agent {
        Agent::Cursor => dest
            .join(".cursor")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
        Agent::Claude => dest
            .join(".claude")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
        Agent::Codex => dest
            .join(".codex")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
        Agent::OpenCode => dest
            .join(".opencode")
            .join("skills")
            .join("lade")
            .join("SKILL.md"),
    }
}

fn agents_skill_file(dir: &Path) -> PathBuf {
    dir.join(".agents")
        .join("skills")
        .join("lade")
        .join("SKILL.md")
}

/// Drop Lade-managed skills under home, and under `dest` when it is a git root.
/// Leaves unmanaged files. A no-git cwd is not `dest`.
pub(super) fn sweep_lade_skills(home: &Path, dest: Option<&Path>) -> Vec<PretoolRow> {
    let mut rows = Vec::new();
    let rel = dest.unwrap_or(home);
    for agent in AGENTS {
        push_removed(&mut rows, &skill_path(agent, home), home, rel);
        if let Some(dest) = dest {
            push_removed(&mut rows, &project_skill_file(agent, dest), home, dest);
        }
    }
    push_removed(&mut rows, &agents_skill_file(home), home, rel);
    if let Some(dest) = dest {
        push_removed(&mut rows, &agents_skill_file(dest), home, dest);
    }
    rows
}

fn push_removed(rows: &mut Vec<PretoolRow>, path: &Path, home: &Path, dest: &Path) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    if !is_lade_skill(&content) {
        return;
    }
    if fs::remove_file(path).is_err() {
        return;
    }
    rows.push(PretoolRow {
        agent: "skill",
        verb: ItemVerb::Removed,
        path: short_path(path, home, dest),
        note: WHY_NO_SKILL,
    });
}
