use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Config;

use super::LADE_MISE_CONFIG;
use super::Outcome;
use super::ensure;
use super::env;
use super::error::Error;
use super::implied;
use super::install;
use super::lock;
use super::lookup;
use super::project;
use super::spec;
use super::store;

pub async fn prepare(
    config: &Config,
    command: &str,
    cwd: &Path,
    saved_user: &Option<String>,
) -> Result<Outcome, Error> {
    let argv0 = spec::argv0(command);
    if let Some(uri) = config.command_package_uri(command, saved_user) {
        return Err(Error::box_lines([
            format!("{uri} is a setup package, not command access."),
            String::new(),
            "Put apm:// and skills:// on `.` (or another setup rule), not on the command."
                .to_string(),
        ]));
    }
    let pins = pins_from(config, saved_user)?;
    if spec::is_mise_argv0(argv0) {
        return intercept_mise(cwd, &pins);
    }
    let mut out = Outcome::default();
    if let Some((key, spec)) = pins.iter().find(|(key, _)| key == argv0) {
        out = pin_command(cwd, key, spec.clone(), true).await?;
    } else if let Some(value) = config.bare_version_for(command, argv0, saved_user) {
        return Err(Error::bare_version(argv0, &value));
    }
    let sources = config.sources_for_command(command, saved_user);
    for (key, spec) in implied::pins_for(&sources, &pins) {
        let extra = pin_command(cwd, &key, spec, false).await?;
        merge_outcome(&mut out, extra);
    }
    Ok(out)
}

fn merge_outcome(into: &mut Outcome, extra: Outcome) {
    if let Some(path) = extra.env.get("PATH") {
        let first = std::env::split_paths(path)
            .next()
            .unwrap_or_else(|| PathBuf::from("."));
        let current = into
            .env
            .get("PATH")
            .cloned()
            .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
        let mut paths = vec![first];
        paths.extend(std::env::split_paths(&current));
        if let Ok(joined) = std::env::join_paths(paths) {
            into.env
                .insert("PATH".to_string(), joined.to_string_lossy().into_owned());
        }
    }
    for (key, value) in extra.env {
        if key == "PATH" {
            continue;
        }
        into.env.entry(key).or_insert(value);
    }
    into.cleanup.extend(extra.cleanup);
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

async fn pin_command(
    cwd: &Path,
    key: &str,
    spec: spec::Spec,
    allow_install: bool,
) -> Result<Outcome, Error> {
    let installs = store::installs_dir();
    let names = store::tool_names(&spec, key, None);
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let found = lock::slot_and_path(cwd, &name_refs);
    let lock_ok = found
        .as_ref()
        .is_some_and(|(_, slot)| lock::agrees(slot, &spec.version, &spec.backend_id()));
    let names = store::tool_names(
        &spec,
        key,
        found.as_ref().map(|(_, slot)| slot.name.as_str()),
    );
    let lookup_version = if lock_ok {
        found
            .as_ref()
            .map(|(_, slot)| slot.version.clone())
            .unwrap_or_else(|| spec.version.clone())
    } else if spec.is_range() {
        lookup::matching_store_version(key, &spec, &installs, &names)
            .unwrap_or_else(|| spec.version.clone())
    } else {
        spec.version.clone()
    };
    if let Some(bin_dir) = lookup::find_cli_dir(&installs, &names, &lookup_version, key, &spec) {
        rewrite_floating_yaml(cwd, key, &spec, &lookup_version)?;
        return activate(bin_dir, &spec, &installs, cwd, &lookup_version).await;
    }
    if !allow_install {
        return Err(Error::box_lines([
            format!("locked {key} is missing."),
            String::new(),
            "Run `lade setup` to install the pinned CLI.".to_string(),
            "Homebrew or another PATH binary is not used.".to_string(),
        ]));
    }
    ensure::require_for_inject().await?;
    if lock_ok {
        if let Some((path, slot)) = found.as_ref() {
            install::install_locked(&slot.name, path, &spec, &installs, cwd).await?;
        } else {
            install::install_from_url(&spec, &installs, cwd).await?;
        }
    } else {
        install::install_from_url(&spec, &installs, cwd).await?;
        rewrite_nearest_lock(cwd, key, &spec)?;
    }
    let found_version = if lock_ok {
        found
            .as_ref()
            .map(|(_, slot)| slot.version.clone())
            .unwrap_or_else(|| spec.version.clone())
    } else if spec.is_range() {
        lookup::matching_store_version(key, &spec, &installs, &names)
            .unwrap_or_else(|| spec.version.clone())
    } else {
        spec.version.clone()
    };
    match lookup::find_cli_dir(&installs, &names, &found_version, key, &spec) {
        Some(bin_dir) => {
            rewrite_floating_yaml(cwd, key, &spec, &found_version)?;
            activate(bin_dir, &spec, &installs, cwd, &found_version).await
        }
        None => Err(Error::refuse(key, &spec.cli_spec())),
    }
}

fn rewrite_floating_yaml(
    cwd: &Path,
    key: &str,
    spec: &spec::Spec,
    resolved: &str,
) -> Result<(), Error> {
    if !spec::version_is_floating(&spec.version) || resolved == spec.version {
        return Ok(());
    }
    let Some(dir) = lookup::yaml_dirs(cwd)?.into_iter().next() else {
        return Ok(());
    };
    let Some(path) = lookup::yaml_file_in(&dir)? else {
        return Ok(());
    };
    let uri = spec::replace_version(&spec.uri, resolved);
    crate::add::replace_binding_uri(&path, key, &uri).map_err(|e| Error::install(e.to_string()))?;
    Ok(())
}

fn rewrite_nearest_lock(cwd: &Path, key: &str, spec: &spec::Spec) -> Result<(), Error> {
    let Some(dir) = lookup::yaml_dirs(cwd)?.into_iter().next() else {
        return Ok(());
    };
    let path = dir.join("lade.lock");
    let mut slots = lock::read_tools(&path).unwrap_or_default();
    lock::upsert(
        &mut slots,
        lookup::slot_after_install(key, spec, &store::installs_dir()),
    );
    lock::write_tools(&path, &slots).map_err(|e| Error::install(e.to_string()))
}

fn spec_for_env(spec: &spec::Spec, resolved: &str) -> spec::Spec {
    if !spec.is_range() || resolved == spec.version {
        return spec.clone();
    }
    spec::Spec {
        prefix: spec.prefix.clone(),
        package: spec.package.clone(),
        options: spec.options.clone(),
        version: resolved.to_string(),
        uri: format!("mise://{}/{}@{resolved}", spec.prefix, spec.package),
    }
}

async fn activate(
    bin_dir: PathBuf,
    spec: &spec::Spec,
    installs: &Path,
    cwd: &Path,
    resolved: &str,
) -> Result<Outcome, Error> {
    let env_spec = spec_for_env(spec, resolved);
    let extra = match env::load(&env_spec) {
        Some(map) => map,
        None => env::refresh(&env_spec, installs, cwd).await?,
    };
    let mut env = extra;
    env.insert("PATH".to_string(), prepend_path(&bin_dir));
    Ok(Outcome {
        env,
        cleanup: Vec::new(),
    })
}

pub fn locked_bin(cwd: Option<&Path>, key: &str, spec: &spec::Spec) -> Option<PathBuf> {
    let installs = store::installs_dir();
    let names = store::tool_names(spec, key, None);
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let version = cwd
        .and_then(|cwd| lock::slot_for_walk(cwd, &name_refs).map(|slot| slot.version))
        .or_else(|| {
            if spec.is_range() {
                lookup::argv0s(key, spec).into_iter().find_map(|argv0| {
                    store::resolve_matching_version(&installs, &names, argv0, &spec.version)
                })
            } else if lookup::find_cli_dir(&installs, &names, &spec.version, key, spec).is_some() {
                Some(spec.version.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| spec.version.clone());
    lookup::find_cli_file(&installs, &names, &version, key, spec)
}

pub fn locked_cli_bin(cli: &str) -> Option<PathBuf> {
    let row = implied::by_key(cli).or_else(|| implied::by_scheme(cli))?;
    let spec = spec::parse(&row.uri).ok()?;
    let cwd = std::env::current_dir().ok();
    locked_bin(cwd.as_deref(), row.key, &spec)
}

pub fn implied_bin(uri: &str) -> Option<&'static str> {
    uri.split_once("://")
        .and_then(|(scheme, _)| implied::by_scheme(scheme).map(|row| row.key))
}

#[derive(Debug, Clone)]
pub struct LockedTool {
    pub name: String,
    pub version: Option<String>,
    pub present: bool,
}

pub fn locked_tools(cwd: &Path, config: &Config, saved: &Option<String>) -> Vec<LockedTool> {
    let Ok(explicit) = pins_from(config, saved) else {
        return Vec::new();
    };
    let mut sources = config.all_secret_sources(saved);
    sources.extend(config.all_network_sources(saved));
    sources.extend(config.package_uris(saved).into_iter().map(|(_, uri)| uri));
    let mut pins = explicit;
    for (key, spec) in implied::pins_for(&sources, &pins) {
        pins.push((key, spec));
    }
    let installs = store::installs_dir();
    let mut out = Vec::new();
    for (key, spec) in pins {
        let names = store::tool_names(&spec, &key, None);
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let slot = lock::slot_for_walk(cwd, &name_refs);
        let version = slot
            .as_ref()
            .map(|s| s.version.clone())
            .unwrap_or_else(|| spec.version.clone());
        let tool_names = store::tool_names(&spec, &key, slot.as_ref().map(|s| s.name.as_str()));
        let present = lookup::find_cli_dir(&installs, &tool_names, &version, &key, &spec).is_some();
        out.push(LockedTool {
            name: key,
            version: Some(version),
            present,
        });
    }
    out
}

fn intercept_mise(cwd: &Path, pins: &[(String, spec::Spec)]) -> Result<Outcome, Error> {
    if pins.is_empty() {
        return Ok(Outcome::default());
    }
    let body = project::compose_toml(&[], pins);
    std::fs::create_dir_all(crate::ticket::dir()).map_err(|e| Error::install(e.to_string()))?;
    let path = crate::ticket::dir().join(format!("lade-mise-{}.toml", crate::ticket::new_id()));
    std::fs::write(&path, body).map_err(|e| Error::install(e.to_string()))?;
    let ignored = project::isolate_config_paths(cwd);
    let mut env = HashMap::new();
    env.insert(
        "MISE_GLOBAL_CONFIG_FILE".to_string(),
        path.to_string_lossy().to_string(),
    );
    env.insert(
        "MISE_CONFIG_DIR".to_string(),
        crate::ticket::dir().to_string_lossy().to_string(),
    );
    env.insert(
        "MISE_TRUSTED_CONFIG_PATHS".to_string(),
        crate::ticket::dir().to_string_lossy().to_string(),
    );
    if !ignored.is_empty() {
        let joined = std::env::join_paths(&ignored).unwrap_or_default();
        env.insert(
            "MISE_IGNORED_CONFIG_PATHS".to_string(),
            joined.to_string_lossy().to_string(),
        );
    }
    env.insert(
        LADE_MISE_CONFIG.to_string(),
        path.to_string_lossy().to_string(),
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
