use std::path::{Path, PathBuf};

use crate::message_box::MessageBox;

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
    directories::ProjectDirs::from("com", "zifeo", "lade")
        .map(|project| project.data_local_dir().join("mise/bin/mise"))
        .unwrap_or_else(|| PathBuf::from(".lade-mise/mise"))
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

pub fn mise_program() -> PathBuf {
    for bin in candidate_bins() {
        if version_of_blocking(&bin).is_some_and(|status| status.in_range) {
            return bin;
        }
    }
    candidate_bins()
        .into_iter()
        .find(|bin| bin.is_file())
        .unwrap_or_else(|| PathBuf::from("mise"))
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
    let saved = crate::global_config::GlobalConfig::user_from_disk();
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
    let status = path_status().await;
    if status.in_range {
        return Ok(());
    }
    match fetch::fetch_official(managed_bin()).await {
        Ok(()) => {
            let again = path_status().await;
            if again.in_range {
                MessageBox::new()
                    .info()
                    .line(format!(
                        "Using mise {} ({}).",
                        again.version.unwrap_or_default(),
                        range::MISE_RANGE
                    ))
                    .print_stderr();
                return Ok(());
            }
            Err(Error::missing_mise(format!(
                "{}. Fetched mise is still outside the range.",
                mise_gap(&again)
            )))
        }
        Err(detail) => Err(Error::missing_mise(format!(
            "{}. {detail}",
            mise_gap(&status)
        ))),
    }
}

pub async fn require_for_inject() -> Result<(), Error> {
    let status = path_status().await;
    if status.in_range {
        return Ok(());
    }
    Err(Error::missing_mise(format!(
        "{}. Inject does not fetch mise.",
        mise_gap(&status)
    )))
}
