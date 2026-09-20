use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::{Audience, Config};

use super::LADE_MISE_CONFIG;
use super::Outcome;
use super::error::Error;
use super::implied;
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
    audience: Audience,
) -> Result<Outcome, Error> {
    let argv0 = spec::argv0(command);
    if let Some(uri) = config.command_package_uri(command, saved_user, audience) {
        return Err(Error::box_lines([
            format!("{uri} is a setup package, not command access."),
            String::new(),
            "Put apm:// and skills:// on `.` (or another setup rule), not on the command."
                .to_string(),
        ]));
    }
    let pins = pins_from(config, command, saved_user, audience)?;
    if spec::is_mise_argv0(argv0) {
        return intercept_mise(cwd, &pins);
    }
    let mut out = Outcome::default();
    let mut applied_argv0 = false;
    if let Some((key, spec)) = pins.iter().find(|(key, _)| key == argv0) {
        out = super::pin::pin_command(cwd, key, spec.clone(), true).await?;
        applied_argv0 = true;
    } else if let Some(value) = config.bare_version_for(command, argv0, saved_user, audience) {
        return Err(Error::bare_version(argv0, &value));
    }
    let sources = config.sources_for_command(command, saved_user, audience);
    for (key, spec) in implied::pins_for(&sources, &[]) {
        if applied_argv0 && key == argv0 {
            continue;
        }
        let (spec, allow_install) = pins
            .iter()
            .find(|(pin_key, _)| pin_key == &key)
            .map(|(_, yaml)| (yaml.clone(), true))
            .unwrap_or((spec, false));
        let extra = super::pin::pin_command(cwd, &key, spec, allow_install).await?;
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
    command: &str,
    saved_user: &Option<String>,
    audience: Audience,
) -> Result<Vec<(String, spec::Spec)>, Error> {
    parse_pins(config.pins_for_command(command, saved_user, audience))
}

fn parse_pins(pins: Vec<(String, String)>) -> Result<Vec<(String, spec::Spec)>, Error> {
    let mut out = Vec::new();
    for (key, value) in pins {
        match spec::parse(&value) {
            Ok(parsed) => out.push((key, parsed)),
            Err(detail) => return Err(Error::invalid_spec(detail)),
        }
    }
    Ok(out)
}

pub fn implied_package(uri: &str) -> Option<&'static str> {
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
    let Ok(explicit) = parse_pins(config.pins(saved)) else {
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
