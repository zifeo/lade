mod ensure;
mod env;
mod error;
mod fetch;
mod implied;
mod install;
mod lifecycle;
mod lock;
mod lookup;
mod pin;
mod plane;
mod prepare;
mod project;
mod range;
mod run;
mod setup;
mod spec;
mod store;
mod toml_merge;
mod walk;

#[cfg(test)]
mod tests;

pub use ensure::{ensure_for_setup, managed_mise_in_play, mise_program, status_info};
pub use error::Error;
pub use lifecycle::run_lifecycle_commands;
pub use pin::{locked_bin, locked_cli_bin};
pub use plane::scan;
pub use prepare::{implied_package, locked_tools, prepare};
pub use run::run_in_repo;
pub use setup::{PinMode, setup_pins};
pub use spec::{
    argv0, is_mise_argv0, looks_like_bare_version, looks_like_spec, parse as parse_spec,
};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn lock_path_in(start: &Path) -> PathBuf {
    match scan(start).lock_path() {
        Some(path) => path.to_path_buf(),
        None => lock::path_in(start),
    }
}

pub fn lock_slot(path: &Path, name: &str) -> Option<String> {
    lock::slot_for(path, &[name]).map(|slot| slot.version)
}

pub const LADE_MISE_CONFIG: &str = "LADE_MISE_CONFIG";

pub fn pin_exact(uri: &str) -> anyhow::Result<String> {
    if !uri.starts_with("mise://") {
        return Ok(uri.to_string());
    }
    let spec = spec::parse(uri).map_err(Error::invalid_spec)?;
    if !spec::version_is_floating(&spec.version) {
        return Ok(uri.to_string());
    }
    let version = latest_matching(&spec)?;
    if version.is_empty() {
        anyhow::bail!("mise latest returned nothing for {}", spec.backend_id());
    }
    Ok(spec::replace_version(uri, &version))
}

pub(super) fn latest_matching(spec: &spec::Spec) -> anyhow::Result<String> {
    let query = if spec::version_is_range(&spec.version) {
        format!("{}@{}", spec.backend_id(), spec.version)
    } else {
        spec.backend_id()
    };
    let output = std::process::Command::new(ensure::mise_program())
        .args(["latest", &query])
        .output()
        .map_err(|e| Error::missing_mise(e.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("mise latest {query} failed: {stderr}");
    }
    Ok(parse_latest_stdout(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_latest_stdout(stdout: &str) -> String {
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub env: HashMap<String, String>,
    pub cleanup: Vec<PathBuf>,
}

impl Outcome {
    pub fn is_empty(&self) -> bool {
        self.env.is_empty() && self.cleanup.is_empty()
    }
}

pub fn unlink_config(path: &Path) {
    if path.is_file() {
        let _ = std::fs::remove_file(path);
    }
}
