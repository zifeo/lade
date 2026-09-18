use std::path::{Path, PathBuf};

use super::error::Error;
use super::lock;
use super::spec;
use super::store;

pub fn upsert_pin(pins: &mut Vec<(String, spec::Spec)>, key: String, spec: spec::Spec) {
    if let Some((existing_key, existing)) = pins
        .iter_mut()
        .find(|(k, s)| k == &key || s.short_name() == spec.short_name())
    {
        *existing_key = key;
        *existing = spec;
        return;
    }
    pins.push((key, spec));
}

pub fn matching_store_version(
    key: &str,
    spec: &spec::Spec,
    installs: &Path,
    names: &[String],
) -> Option<String> {
    argv0s(key, spec)
        .into_iter()
        .find_map(|argv0| store::resolve_matching_version(installs, names, argv0, &spec.version))
}

pub fn slot_after_install(key: &str, spec: &spec::Spec, installs: &Path) -> lock::LockSlot {
    let names = store::tool_names(spec, key, None);
    let version = if spec.is_range() {
        matching_store_version(key, spec, installs, &names).unwrap_or_else(|| spec.version.clone())
    } else {
        spec.version.clone()
    };
    let checksum = find_cli_file(installs, &names, &version, key, spec)
        .and_then(|bin| lock::file_checksum(&bin));
    lock::LockSlot {
        name: key.to_string(),
        version,
        backend: Some(spec.backend_id()),
        checksum,
    }
}

pub fn argv0s<'a>(key: &'a str, spec: &'a spec::Spec) -> Vec<&'a str> {
    let mut out = vec![key];
    if spec.short_name() != key {
        out.push(spec.short_name());
    }
    out
}

pub fn find_cli_dir(
    installs: &Path,
    names: &[String],
    version: &str,
    key: &str,
    spec: &spec::Spec,
) -> Option<PathBuf> {
    argv0s(key, spec)
        .into_iter()
        .find_map(|argv0| store::find_pinned_bin(installs, names, version, argv0))
}

pub fn find_cli_file(
    installs: &Path,
    names: &[String],
    version: &str,
    key: &str,
    spec: &spec::Spec,
) -> Option<PathBuf> {
    argv0s(key, spec).into_iter().find_map(|argv0| {
        store::find_pinned_bin(installs, names, version, argv0).map(|dir| dir.join(argv0))
    })
}

pub fn yaml_file_in(dir: &Path) -> Result<Option<PathBuf>, Error> {
    crate::config::config_in_dir(dir).map_err(|e| Error::install(e.to_string()))
}

pub fn yaml_dirs(start: &Path) -> Result<Vec<PathBuf>, Error> {
    crate::config::yaml_files_on_walk(start)
        .map_err(|e| Error::install(e.to_string()))
        .map(|files| {
            files
                .into_iter()
                .filter_map(|file| file.parent().map(Path::to_path_buf))
                .collect()
        })
}
