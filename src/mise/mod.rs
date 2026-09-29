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
mod resolve;
mod run;
mod setup;
mod spec;
mod store;
mod toml_merge;
mod unlistable;
mod walk;

#[cfg(test)]
mod tests;

pub use ensure::{ensure_for_setup, managed_mise_in_play, mise_program, status_info};
pub use error::Error;
pub use implied::repo_needs_mise;
pub use lifecycle::run_lifecycle_commands;
pub use pin::{locked_bin, locked_cli_bin};
pub use plane::scan;
pub use prepare::{implied_package, locked_tools, prepare};
pub use run::run_in_repo;
pub use setup::{PinMode, print_pin_update, setup_pins};
pub use spec::{
    argv0, is_user_shell, looks_like_bare_version, looks_like_spec, parse as parse_spec,
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
    let query = spec.latest_query();
    if spec::version_is_range(&spec.version) {
        let versions = mise_lines(&["ls-remote", &query])?;
        return Ok(resolve::highest_matching(&spec.version, &versions).unwrap_or_default());
    }
    let lines = mise_lines(&["latest", &query])?;
    Ok(lines.into_iter().next().unwrap_or_default())
}

fn mise_lines(args: &[&str]) -> anyhow::Result<Vec<String>> {
    let output = std::process::Command::new(ensure::mise_program())
        .args(args)
        .env("MISE_YES", "1")
        .output()
        .map_err(|e| Error::missing_mise(e.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("mise {} failed: {stderr}", args.join(" "));
    }
    Ok(version_tokens(&String::from_utf8_lossy(&output.stdout)))
}

fn version_tokens(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| {
            let token = line.split_whitespace().next()?.trim();
            if token.is_empty() {
                None
            } else {
                Some(token.to_string())
            }
        })
        .collect()
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
    if crate::cache::is_persistent_mise_config(path) {
        return;
    }
    if path.is_file() {
        let _ = std::fs::remove_file(path);
    }
}
