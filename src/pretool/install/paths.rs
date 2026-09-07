use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::agent::Agent;

pub(super) fn home_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    if let Some(home) = std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("cannot determine home directory")
}

pub(super) fn install_bin() -> String {
    let bin = crate::pretool::invoked_lade_bin();
    match Path::new(&bin).file_name().and_then(|name| name.to_str()) {
        Some("lade" | "lade.exe") => bin,
        _ => "lade".to_string(),
    }
}

pub(super) fn hook_command(agent: Agent) -> String {
    format!("{} hook --harness {}", install_bin(), agent.slug())
}

pub(super) fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ItemVerb {
    Current,
    Installed,
    Updated,
    Removed,
    Unmanaged,
    Stale,
    Missing,
}

impl ItemVerb {
    pub(super) fn label(self) -> &'static str {
        match self {
            ItemVerb::Current => "current",
            ItemVerb::Installed => "installed",
            ItemVerb::Updated => "updated",
            ItemVerb::Removed => "removed",
            ItemVerb::Unmanaged => "skipped",
            ItemVerb::Stale => "stale",
            ItemVerb::Missing => "missing",
        }
    }
}

pub(super) struct WriteOutcome {
    pub verb: ItemVerb,
    pub path: PathBuf,
}

/// Repo-relative when `path` is under `dest`, otherwise `~/…`.
pub(super) fn short_path(path: &Path, home: &Path, dest: &Path) -> String {
    if let Ok(rest) = path.strip_prefix(dest)
        && !rest.as_os_str().is_empty()
    {
        return rest.display().to_string();
    }
    tilde(path, home)
}
