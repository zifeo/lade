use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::ensure;
use super::error::Error;
use super::implied;
use super::install;
use super::lock::{self, LockSlot};
use super::lookup;
use super::plane::{self, Plane, Snapshot};
use super::spec::{self, Spec};
use super::store;
use super::toml_merge;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinMode {
    Locked,
    Unlock,
    Update,
}

pub async fn setup_pins(mode: PinMode) -> anyhow::Result<()> {
    let cwd = std::env::current_dir().map_err(|e| Error::install(e.to_string()))?;
    let config = crate::config::LadeFile::build(cwd.clone())?;
    let saved = crate::global_config::GlobalConfig::user_from_disk();
    if !implied::repo_needs_mise(&config, &saved) {
        if mode == PinMode::Update {
            crate::message_box::MessageBox::new()
                .info()
                .line("No pins in this repo.")
                .print_stderr();
        }
        return Ok(());
    }
    ensure::ensure_for_setup().await.inspect_err(|e| {
        e.emit();
    })?;
    let snap = plane::scan(&cwd);
    warn_plane(&snap);
    let installs = store::installs_dir();
    let mut dirs = snap.yaml_dirs();
    dirs.reverse();
    let mut accumulated: Vec<(String, Spec)> = Vec::new();
    let mut key_dir: HashMap<String, PathBuf> = HashMap::new();
    for dir in dirs {
        for (key, value) in config.pins_in_dir(&dir, &saved) {
            let spec = spec::parse(&value).map_err(Error::invalid_spec)?;
            lookup::upsert_pin(&mut accumulated, key.clone(), spec);
            key_dir.insert(key, dir.clone());
        }
        let implied_pins = implied::pins_for(&config.sources_in_dir(&dir, &saved), &accumulated);
        for (key, spec) in implied_pins {
            lookup::upsert_pin(&mut accumulated, key.clone(), spec);
            key_dir.entry(key).or_insert_with(|| dir.clone());
        }
    }
    if accumulated.is_empty() {
        return Ok(());
    }
    let lock_path = snap.lock_path().map(Path::to_path_buf);
    let mut bumped = Vec::new();
    let mut slots = Vec::new();
    let pending = accumulated.clone();
    for (key, spec) in &pending {
        let names = store::tool_names(spec, key, None);
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let existing = lock_path.as_ref().and_then(|path| {
            path.is_file()
                .then(|| lock::slot_for(path, &name_refs).map(|slot| (path.clone(), slot)))
                .flatten()
        });
        let spec = match mode {
            PinMode::Update => {
                let next = resolve_for_update(key, spec)?;
                let old = existing
                    .as_ref()
                    .map(|(_, slot)| slot.version.as_str())
                    .unwrap_or("");
                if old != next.version {
                    bumped.push((key.clone(), old.to_string(), next.version.clone()));
                }
                next
            }
            PinMode::Unlock => spec.clone(),
            PinMode::Locked => heal_from_toml(&snap, mode, key, spec),
        };
        let lock_ok = mode != PinMode::Unlock
            && existing.as_ref().is_some_and(|(_, slot)| {
                lock::agrees(slot, &spec.version, &spec.backend_id())
                    && (spec.is_range()
                        || spec::version_is_floating(&spec.version)
                        || spec.version == slot.version)
            });
        if lock_ok && let Some((path, slot)) = existing.as_ref() {
            install::install_locked(&slot.name, path, &spec, &installs, &cwd).await?;
            lookup::upsert_pin(&mut accumulated, key.clone(), spec_at_slot(&spec, slot));
            slots.push(slot.clone());
            continue;
        }
        install::install_from_url(&spec, &installs, &cwd).await?;
        let slot = lookup::slot_after_install(key, &spec, &installs);
        if spec::version_is_floating(&spec.version) && slot.version != spec.version {
            let uri = spec::replace_version(&spec.uri, &slot.version);
            if let Some(dir) = key_dir.get(key)
                && let Some(path) = lookup::yaml_file_in(dir)?
            {
                crate::add::replace_binding_uri(&path, key, &uri)
                    .map_err(|e| Error::install(e.to_string()))?;
            }
        }
        lookup::upsert_pin(&mut accumulated, key.clone(), spec_at_slot(&spec, &slot));
        slots.push(slot);
    }
    if let Some(lock_path) = lock_path {
        lock::write_tools(&lock_path, &slots).map_err(|e| Error::install(e.to_string()))?;
    }
    if let Plane::Mise { toml_write, .. } = &snap.plane {
        let entries: Vec<(String, String)> = slots
            .iter()
            .map(|slot| (slot.name.clone(), slot.version.clone()))
            .collect();
        toml_merge::upsert_tools(toml_write, &entries).map_err(Error::install)?;
    }
    if mode == PinMode::Update {
        print_update(&bumped);
    }
    Ok(())
}

fn heal_from_toml(snap: &Snapshot, mode: PinMode, key: &str, spec: &Spec) -> Spec {
    if mode == PinMode::Unlock {
        return spec.clone();
    }
    let Plane::Mise {
        toml, toml_write, ..
    } = &snap.plane
    else {
        return spec.clone();
    };
    let path = toml.as_ref().unwrap_or(toml_write);
    if !path.is_file() {
        return spec.clone();
    }
    let implied = implied::by_key(key).is_some();
    if !implied && !spec.is_range() {
        return spec.clone();
    }
    let Some(version) = toml_merge::tool_version(path, key)
        .or_else(|| toml_merge::tool_version(path, &spec.backend_id()))
        .or_else(|| toml_merge::tool_version(path, spec.short_name()))
    else {
        return spec.clone();
    };
    let probe = LockSlot {
        name: key.to_string(),
        version: version.clone(),
        backend: Some(spec.backend_id()),
        checksum: None,
    };
    if !lock::agrees(&probe, &spec.version, &spec.backend_id()) {
        return spec.clone();
    }
    spec_at_version(spec, &version)
}

fn spec_at_slot(spec: &Spec, slot: &LockSlot) -> Spec {
    spec_at_version(spec, &slot.version)
}

fn spec_at_version(spec: &Spec, version: &str) -> Spec {
    Spec {
        prefix: spec.prefix.clone(),
        package: spec.package.clone(),
        options: spec.options.clone(),
        version: version.to_string(),
        uri: spec::replace_version(&spec.uri, version),
    }
}

fn resolve_for_update(key: &str, spec: &Spec) -> Result<Spec, Error> {
    let implied = implied::by_key(key).is_some();
    if !implied && !spec.is_range() {
        return Ok(spec.clone());
    }
    let Ok(version) = super::latest_matching(spec) else {
        return Ok(spec.clone());
    };
    if version.is_empty() {
        return Ok(spec.clone());
    }
    Ok(spec_at_version(spec, &version))
}

fn warn_plane(snap: &Snapshot) {
    let mut lines: Vec<String> = Vec::new();
    if snap.split_warning {
        lines.push("mise.lock and mise.toml are in different directories.".to_string());
        lines.push(
            "Lade writes both in the nearer directory. The other file is left as-is.".to_string(),
        );
    }
    for path in &snap.leftover_lade_locks {
        lines.push(format!("Leftover {} is ignored.", path.display()));
    }
    if lines.is_empty() {
        return;
    }
    let mut mb = crate::message_box::MessageBox::new().warning();
    for line in lines {
        mb = mb.line(line);
    }
    mb.print_stderr();
}

fn print_update(bumped: &[(String, String, String)]) {
    let mut mb = crate::message_box::MessageBox::new().info();
    if bumped.is_empty() {
        mb = mb.line("Lock already at the latest matching packages.");
    } else {
        mb = mb.line("Updated the lock and installed the new packages.");
        for (key, from, to) in bumped {
            if from.is_empty() {
                mb = mb.line(format!("  {key}  {to}"));
            } else {
                mb = mb.line(format!("  {key}  {from} -> {to}"));
            }
        }
    }
    mb.print_stderr();
}
