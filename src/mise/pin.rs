use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::Outcome;
use super::ensure;
use super::env;
use super::error::Error;
use super::implied;
use super::install;
use super::lock;
use super::lookup;
use super::spec;
use super::store;

pub(super) async fn pin_command(
    cwd: &Path,
    key: &str,
    spec: spec::Spec,
    allow_install: bool,
    from_yaml: bool,
) -> Result<Outcome, Error> {
    let snap = super::plane::scan(cwd);
    let spec = super::decide::decide(&snap, key, spec, from_yaml)
        .spec()
        .clone();
    let installs = store::installs_dir();
    let names = store::tool_names(&spec, key, None);
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let found = snap.lock_path().and_then(|path| {
        path.is_file()
            .then(|| lock::slot_for(path, &name_refs).map(|slot| (path.to_path_buf(), slot)))
            .flatten()
    });
    let lock_ok = found
        .as_ref()
        .is_some_and(|(_, slot)| lock::agrees(slot, &spec.version, &spec.backend_id()));
    let names = store::tool_names(
        &spec,
        key,
        found.as_ref().map(|(_, slot)| slot.name.as_str()),
    );
    let resolve_version = || {
        if lock_ok {
            // § byd2nhybz.e@em6bx
            found
                .as_ref()
                .map(|(_, slot)| slot.version.clone())
                .unwrap_or_else(|| spec.version.clone())
        } else if spec.is_range() {
            lookup::matching_store_version(key, &spec, &installs, &names)
                .unwrap_or_else(|| spec.version.clone())
        } else {
            spec.version.clone()
        }
    };
    let lookup_version = resolve_version();
    if let Some(bin_dir) = lookup::find_cli_dir(&installs, &names, &lookup_version, key) {
        rewrite_floating_yaml(cwd, key, &spec, &lookup_version)?;
        return activate(bin_dir, &spec, &installs, cwd, &lookup_version, false).await;
    }
    if !allow_install {
        // § rtcp7pj7e.b@dr1e6
        return Err(Error::box_lines([
            format!("locked {key} is missing."),
            String::new(),
            "Run `lade setup` to install the pinned CLI.".to_string(),
            "Homebrew or another PATH binary is not used.".to_string(),
        ]));
    }
    ensure::require_for_inject().await?;
    // § rtcp7pj7e.a@vgbqe
    let installed_version = if let (true, Some((path, slot))) = (lock_ok, found.as_ref()) {
        let locked_spec = spec::at_version(&spec, &slot.version);
        if lock::is_generated(path) {
            install::install_locked(path, &locked_spec, &installs, cwd).await?;
        } else {
            install::install_from_url(&locked_spec, &installs, cwd).await?;
        }
        None
    } else {
        let installed = if spec.is_range() {
            let version = super::resolve::concrete_version(&spec)?;
            spec::at_version(&spec, &version)
        } else {
            spec.clone()
        };
        install::install_from_url(&installed, &installs, cwd).await?;
        Some(installed.version)
    };
    let found_version = match installed_version {
        Some(version) => version,
        None => resolve_version(),
    };
    match lookup::find_cli_dir(&installs, &names, &found_version, key) {
        Some(bin_dir) => {
            rewrite_floating_yaml(cwd, key, &spec, &found_version)?;
            activate(bin_dir, &spec, &installs, cwd, &found_version, true).await
        }
        None => Err(Error::refuse(key, &spec.cli_spec())),
    }
    // § end rtcp7pj7e.a
}

fn rewrite_floating_yaml(
    cwd: &Path,
    key: &str,
    spec: &spec::Spec,
    resolved: &str,
) -> Result<(), Error> {
    // § h01jcjwrv.c@y3j35
    if !spec::version_is_floating(&spec.version) || resolved == spec.version {
        return Ok(());
    }
    let uri = spec::replace_version(&spec.uri, resolved);
    // The pin may come from any yaml of the walk, not only the nearest.
    for dir in lookup::yaml_dirs(cwd)? {
        let Some(path) = lookup::yaml_file_in(&dir)? else {
            continue;
        };
        if crate::command::add::replace_binding_value(&path, key, &spec.uri, &uri)
            .map_err(|e| Error::install(e.to_string()))?
        {
            return Ok(());
        }
    }
    Ok(())
}

fn spec_for_env(spec: &spec::Spec, resolved: &str) -> spec::Spec {
    if !spec.is_range() || resolved == spec.version {
        return spec.clone();
    }
    spec::at_version(spec, resolved)
}

async fn activate(
    bin_dir: PathBuf,
    spec: &spec::Spec,
    installs: &Path,
    cwd: &Path,
    resolved: &str,
    capture_env: bool,
) -> Result<Outcome, Error> {
    // § aq3qybz6p.a@xjkwr
    let env_spec = spec_for_env(spec, resolved);
    let extra = match env::load(&env_spec) {
        Some(map) => map,
        None => match env::refresh(&env_spec, installs, cwd).await {
            Ok(map) => map,
            Err(err) if capture_env => return Err(err),
            Err(_) => HashMap::new(),
        },
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
                lookup::matching_store_version(key, spec, &installs, &names)
            } else if lookup::find_cli_dir(&installs, &names, &spec.version, key).is_some() {
                Some(spec.version.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| spec.version.clone());
    lookup::find_cli_file(&installs, &names, &version, key)
}

pub fn locked_cli_bin(cli: &str) -> Option<PathBuf> {
    let row = implied::by_key(cli).or_else(|| implied::by_scheme(cli))?;
    let spec = spec::parse(&row.uri).ok()?;
    let cwd = std::env::current_dir().ok();
    locked_bin(cwd.as_deref(), row.key, &spec)
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
