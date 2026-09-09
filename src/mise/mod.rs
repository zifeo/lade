mod env;
mod error;
mod install;
mod lock;
mod project;
mod spec;
mod store;
mod walk;

#[cfg(test)]
mod tests;

pub use error::Error;
pub use spec::{argv0, is_mise_argv0, looks_like_bare_version, looks_like_spec};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Config;

pub const LADE_MISE_CONFIG: &str = "LADE_MISE_CONFIG";

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

pub async fn prepare(
    config: &Config,
    command: &str,
    cwd: &Path,
    saved_user: &Option<String>,
) -> Result<Outcome, Error> {
    let argv0 = spec::argv0(command);
    let pins = pins_from(config, saved_user)?;
    if spec::is_mise_argv0(argv0) {
        return intercept_mise(cwd, &pins);
    }
    let Some((key, spec)) = pins.iter().find(|(key, _)| key == argv0) else {
        if let Some(value) = config.bare_version_for(command, argv0, saved_user) {
            return Err(Error::bare_version(argv0, &value));
        }
        return Ok(Outcome::default());
    };
    let spec = spec.clone();
    let key = key.clone();
    ensure_no_conflict(cwd, &spec, &key)?;
    pin_command(cwd, argv0, &key, spec).await
}

fn pins_from(
    config: &Config,
    saved_user: &Option<String>,
) -> Result<Vec<(String, spec::Spec)>, Error> {
    let mut out = Vec::new();
    for (key, value) in config.pins(saved_user) {
        match spec::parse(&value) {
            Ok(parsed) => out.push((key, parsed)),
            Err(detail) => return Err(Error::invalid_spec(detail)),
        }
    }
    Ok(out)
}

fn ensure_no_conflict(cwd: &Path, spec: &spec::Spec, key: &str) -> Result<(), Error> {
    let Some(path) = project::find_mise_toml(cwd) else {
        return Ok(());
    };
    let theirs = project::project_tools(&path);
    if let Some(hit) = project::conflict(spec, key, &theirs) {
        return Err(Error::conflict(
            key,
            &hit.version,
            &spec.version,
            &path.display().to_string(),
        ));
    }
    Ok(())
}

async fn pin_command(
    cwd: &Path,
    argv0: &str,
    key: &str,
    spec: spec::Spec,
) -> Result<Outcome, Error> {
    let installs = store::installs_dir();
    let lock_path = lock::find_lock(cwd);
    let lock_names = [
        spec.short_name().to_string(),
        key.to_string(),
        spec.backend_id(),
        spec.backend_slug(),
    ];
    let name_refs: Vec<&str> = lock_names.iter().map(String::as_str).collect();
    let slot = lock_path
        .as_ref()
        .and_then(|path| lock::slot_for(path, &name_refs));
    let lock_ok = slot
        .as_ref()
        .is_some_and(|slot| lock::agrees(slot, &spec.version, &spec.backend_id()));
    let names = store::tool_names(&spec, key, slot.as_ref().map(|s| s.name.as_str()));
    if let Some(bin_dir) = store::find_pinned_bin(&installs, &names, &spec.version, argv0) {
        return activate(bin_dir, &spec, &installs, cwd).await;
    }
    if lock_ok {
        if let Some(path) = lock_path.as_ref()
            && let Some(slot) = slot.as_ref()
        {
            install::install_locked(&slot.name, path, &spec, &installs, cwd).await?;
        } else {
            install::install_from_url(&spec, &installs, cwd).await?;
        }
    } else {
        install::install_from_url(&spec, &installs, cwd).await?;
    }
    match store::find_pinned_bin(&installs, &names, &spec.version, argv0) {
        Some(bin_dir) => activate(bin_dir, &spec, &installs, cwd).await,
        None => Err(Error::refuse(argv0, &spec.cli_spec())),
    }
}

async fn activate(
    bin_dir: PathBuf,
    spec: &spec::Spec,
    installs: &Path,
    cwd: &Path,
) -> Result<Outcome, Error> {
    let extra = match env::load(spec) {
        Some(map) => map,
        None => env::refresh(spec, installs, cwd).await?,
    };
    let mut env = extra;
    env.insert("PATH".to_string(), prepend_path(&bin_dir));
    Ok(Outcome {
        env,
        cleanup: Vec::new(),
    })
}

fn intercept_mise(cwd: &Path, pins: &[(String, spec::Spec)]) -> Result<Outcome, Error> {
    if pins.is_empty() {
        return Ok(Outcome::default());
    }
    for (key, spec) in pins {
        ensure_no_conflict(cwd, spec, key)?;
    }
    let theirs = project::find_mise_toml(cwd)
        .map(|path| project::project_tools(&path))
        .unwrap_or_default();
    let body = project::compose_toml(&theirs, pins);
    std::fs::create_dir_all(crate::ticket::dir()).map_err(|e| Error::install(e.to_string()))?;
    let path = crate::ticket::dir().join(format!("lade-mise-{}.toml", crate::ticket::new_id()));
    std::fs::write(&path, body).map_err(|e| Error::install(e.to_string()))?;
    let ignored = project::isolate_config_paths(cwd);
    let mut env = HashMap::new();
    env.insert(
        "MISE_GLOBAL_CONFIG_FILE".to_string(),
        path.to_string_lossy().into_owned(),
    );
    env.insert(
        "MISE_CONFIG_DIR".to_string(),
        crate::ticket::dir().to_string_lossy().into_owned(),
    );
    env.insert(
        "MISE_TRUSTED_CONFIG_PATHS".to_string(),
        crate::ticket::dir().to_string_lossy().into_owned(),
    );
    if !ignored.is_empty() {
        let joined = std::env::join_paths(&ignored).unwrap_or_default();
        env.insert(
            "MISE_IGNORED_CONFIG_PATHS".to_string(),
            joined.to_string_lossy().into_owned(),
        );
    }
    env.insert(
        LADE_MISE_CONFIG.to_string(),
        path.to_string_lossy().into_owned(),
    );
    Ok(Outcome {
        env,
        cleanup: vec![path],
    })
}

fn prepend_path(bin_dir: &Path) -> String {
    let mut paths = vec![bin_dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths)
        .map(|joined| joined.to_string_lossy().into_owned())
        .unwrap_or_else(|_| bin_dir.display().to_string())
}
