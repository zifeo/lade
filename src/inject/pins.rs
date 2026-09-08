use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::mise::{self, Outcome as PinOutcome};

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
) -> Result<PinOutcome> {
    match mise::prepare(config, command, cwd, saved_user).await {
        Ok(out) => Ok(out),
        Err(error) => {
            error.emit();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    }
}

pub(super) fn select_tool_env(
    env: &mut HashMap<String, String>,
    tool: HashMap<String, String>,
) -> Result<()> {
    for (key, value) in tool {
        if key == "PATH" {
            env.insert(key, value);
            continue;
        }
        match env.get(&key) {
            Some(existing) if existing != &value => {
                anyhow::bail!(
                    "conflicting env '{key}' between lade.yml and the mise pin: '{existing}' vs '{value}'"
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
