use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::mise::{self, Outcome as PinOutcome};
use crate::preexec::Shell;

pub(super) struct PinCleanup(pub(super) Vec<PathBuf>);

impl Drop for PinCleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            mise::unlink_config(path);
        }
    }
}

pub(super) async fn apply_pins(
    config: &Config,
    command: &str,
    cwd: &std::path::Path,
    saved_user: &Option<String>,
    audience: crate::config::Audience,
    preexec: Option<&Shell>,
) -> Result<PinOutcome> {
    match mise::prepare(config, command, cwd, saved_user, audience).await {
        Ok(out) => {
            if let Some(path) = out.env.get("PATH") {
                // Hydrate CLIs and tunnel children inherit process PATH.
                // Command.env covers our children. age plugins and OpenSSH
                // still read the process. Edition 2024 set_var is unsafe.
                unsafe {
                    std::env::set_var("PATH", path);
                }
            }
            Ok(out)
        }
        Err(error) => {
            // § t7kcndtra.a@pes6a
            error.emit();
            match preexec {
                Some(shell) => super::refuse_preexec(shell, crate::exit_codes::FAILURE),
                None => std::process::exit(crate::exit_codes::FAILURE),
            }
        }
    }
}

/// Puts back the `PATH` Lade had before [`apply_pins`], for a command that
/// runs without Lade.
pub(super) fn restore_unpinned_path(path: Option<std::ffi::OsString>) {
    // § ntr20kvnb.a@9nf97
    // SAFETY: as in `apply_pins`. The providers and tunnels are done.
    unsafe {
        match path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

pub(super) fn select_tool_env(
    env: &mut HashMap<String, String>,
    tool: HashMap<String, String>,
) -> Result<()> {
    // A binding named `PATH` conflicts with a pin's `PATH` like any other.
    // § d5fjsqxna.d@82c6a
    for (key, value) in tool {
        match env.get(&key) {
            Some(existing) if existing != &value => {
                // § aq3qybz6p.c@kr6jx
                anyhow::bail!(
                    "conflicting env '{key}': lade.yaml and the mise pin set different values"
                );
            }
            Some(_) => {}
            None => {
                env.insert(key, value);
            }
        }
    }
    Ok(())
}
