use std::path::{Path, PathBuf};

use super::error::Error;
use super::lock;
use super::spec;
use super::store;

pub fn upsert_pin(pins: &mut Vec<(String, spec::Spec)>, key: String, spec: spec::Spec) {
    if let Some((existing_key, existing)) = pins
        .iter_mut()
        .find(|(k, s)| k == &key || s.backend_id() == spec.backend_id())
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
    lock::LockSlot {
        name: key.to_string(),
        version,
        backend: Some(spec.backend_id()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_suffix_does_not_collapse_two_pins() {
        let op = spec::parse("mise://aqua/1password/cli@2.30.0").unwrap();
        let doppler = spec::parse("mise://aqua/DopplerHQ/cli@3.75.1").unwrap();
        let mut pins = Vec::new();
        upsert_pin(&mut pins, "op".to_string(), op);
        upsert_pin(&mut pins, "doppler".to_string(), doppler);
        assert_eq!(pins.len(), 2);
        assert_eq!(pins[0].1.backend_id(), "aqua:1password/cli");
        assert_eq!(pins[1].1.backend_id(), "aqua:DopplerHQ/cli");
    }

    #[test]
    fn same_backend_replaces() {
        let first = spec::parse("mise://aqua/jqlang/jq@1.7.1").unwrap();
        let second = spec::parse("mise://aqua/jqlang/jq@1.8.0").unwrap();
        let mut pins = Vec::new();
        upsert_pin(&mut pins, "jq".to_string(), first);
        upsert_pin(&mut pins, "json".to_string(), second);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].0, "json");
        assert_eq!(pins[0].1.version, "1.8.0");
    }
}
