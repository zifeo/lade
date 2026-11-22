use std::path::{Path, PathBuf};

use super::error::Error;
use super::fetch;
use super::implied;
use super::range::{self, MiseStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusInfo {
    pub needed: bool,
    pub version: Option<String>,
    pub in_range: bool,
    pub range: String,
}

pub fn managed_mise_in_play() -> bool {
    managed_bin().is_file()
}

pub fn managed_bin() -> PathBuf {
    // § kwbgdgf2x.b@2av4h
    super::store::data_dir().join("bin/mise")
}

fn path_mise() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let bin = dir.join("mise");
        bin.is_file().then_some(bin)
    })
}

fn candidate_bins() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(path) = std::env::var("LADE_MISE")
        && !path.is_empty()
    {
        out.push(PathBuf::from(path));
    }
    if let Some(path) = path_mise()
        && !out.contains(&path)
    {
        out.push(path);
    }
    let managed = managed_bin();
    if !out.contains(&managed) {
        out.push(managed);
    }
    out
}

fn version_of_blocking(bin: &Path) -> Option<MiseStatus> {
    let output = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() && output.stdout.is_empty() && output.stderr.is_empty() {
        return None;
    }
    Some(range::status_from_stdout(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub fn mise_program() -> Result<PathBuf, Error> {
    // § kwbgdgf2x.a@8m8st
    let mut first = None;
    for bin in candidate_bins() {
        let Some(status) = version_of_blocking(&bin) else {
            continue;
        };
        if status.in_range {
            return Ok(bin);
        }
        first.get_or_insert(status);
    }
    let status = first.unwrap_or(MiseStatus {
        version: None,
        in_range: false,
    });
    Err(Error::missing_mise(mise_gap(&status)))
}

async fn version_of(bin: &Path) -> Option<MiseStatus> {
    let output = tokio::process::Command::new(bin)
        .arg("--version")
        .output()
        .await
        .ok()?;
    if !output.status.success() && output.stdout.is_empty() && output.stderr.is_empty() {
        return None;
    }
    Some(range::status_from_stdout(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub async fn path_status() -> MiseStatus {
    let mut best = MiseStatus {
        version: None,
        in_range: false,
    };
    for bin in candidate_bins() {
        let Some(status) = version_of(&bin).await else {
            continue;
        };
        if status.in_range {
            return status;
        }
        if best.version.is_none() {
            best = status;
        }
    }
    best
}

pub async fn status_info() -> StatusInfo {
    let needed = repo_needs_mise();
    let status = path_status().await;
    StatusInfo {
        needed,
        version: status.version,
        in_range: status.in_range,
        range: range::MISE_RANGE.to_string(),
    }
}

fn repo_needs_mise() -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    let Ok(config) = crate::config::LadeFile::build(cwd) else {
        return false;
    };
    let Ok(saved) = crate::global_config::GlobalConfig::resolved_user() else {
        return false;
    };
    implied::repo_needs_mise(&config, &saved)
}

fn mise_gap(status: &MiseStatus) -> String {
    if status.version.is_none() {
        format!(
            "mise is not on PATH. This Lade wants {}.",
            range::MISE_RANGE
        )
    } else {
        format!(
            "mise {} is outside {}.",
            status.version.clone().unwrap_or_default(),
            range::MISE_RANGE
        )
    }
}

/// Setup and upgrade may fetch. Inject must not.
pub async fn ensure_for_setup() -> Result<(), Error> {
    if !repo_needs_mise() {
        return Ok(());
    }
    ensure_in_range().await
}

/// After `lade upgrade`, which never reads the walk.
pub async fn ensure_managed() -> Result<(), Error> {
    ensure_in_range().await
}

async fn ensure_in_range() -> Result<(), Error> {
    // § kwbgdgf2x.a@3mee4
    let status = path_status().await;
    if status.in_range {
        crate::live_progress::done(
            "mise",
            crate::live_progress::named_version("mise", &status.version.unwrap_or_default()),
        );
        return Ok(());
    }
    match fetch::fetch_official(managed_bin()).await {
        Ok(()) => {
            let again = path_status().await;
            if again.in_range {
                crate::live_progress::done(
                    "mise",
                    crate::live_progress::named_version("mise", &again.version.unwrap_or_default()),
                );
                return Ok(());
            }
            crate::live_progress::failed("mise", "mise");
            Err(Error::missing_mise(format!(
                "{}. Fetched mise is still outside the range.",
                mise_gap(&again)
            )))
        }
        Err(detail) => {
            crate::live_progress::failed("mise", "mise");
            Err(Error::missing_mise(format!(
                "{}. {detail}",
                mise_gap(&status)
            )))
        }
    }
}

pub async fn require_for_inject() -> Result<(), Error> {
    // § kwbgdgf2x.c@hz66p
    let status = path_status().await;
    if status.in_range {
        return Ok(());
    }
    Err(Error::missing_mise(format!(
        "{}. Inject does not fetch mise.",
        mise_gap(&status)
    )))
}
