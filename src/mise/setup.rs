use super::ensure;
use super::error::Error;
use super::implied;
use super::install;
use super::lock;
use super::lookup;
use super::spec::{self, Spec};
use super::store;

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
    let installs = store::installs_dir();
    let mut dirs = lookup::yaml_dirs(&cwd)?;
    dirs.reverse();
    let mut accumulated: Vec<(String, Spec)> = Vec::new();
    let mut bumped = Vec::new();
    for dir in dirs {
        for (key, value) in config.pins_in_dir(&dir, &saved) {
            let spec = spec::parse(&value).map_err(Error::invalid_spec)?;
            lookup::upsert_pin(&mut accumulated, key, spec);
        }
        let implied_pins = implied::pins_for(&config.sources_in_dir(&dir, &saved), &accumulated);
        for (key, spec) in implied_pins {
            lookup::upsert_pin(&mut accumulated, key, spec);
        }
        if accumulated.is_empty() {
            continue;
        }
        let pending = accumulated.clone();
        let mut slots = Vec::new();
        for (key, spec) in &pending {
            let names = store::tool_names(spec, key, None);
            let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let existing = lock::slot_and_path(&dir, &name_refs);
            if mode == PinMode::Locked
                && let Some((path, slot)) = existing.as_ref()
            {
                install::install_locked(&slot.name, path, spec, &installs, &cwd).await?;
                let pinned = spec_at_slot(spec, slot);
                lookup::upsert_pin(&mut accumulated, key.clone(), pinned);
                slots.push(slot.clone());
                continue;
            }
            let spec = if mode == PinMode::Update {
                let next = resolve_for_update(key, spec)?;
                let old = existing
                    .as_ref()
                    .map(|(_, slot)| slot.version.as_str())
                    .unwrap_or("");
                if old != next.version {
                    bumped.push((key.clone(), old.to_string(), next.version.clone()));
                }
                next
            } else {
                spec.clone()
            };
            install::install_from_url(&spec, &installs, &cwd).await?;
            let slot = lookup::slot_after_install(key, &spec, &installs);
            if spec::version_is_floating(&spec.version) && slot.version != spec.version {
                let uri = spec::replace_version(&spec.uri, &slot.version);
                if let Some(path) = lookup::yaml_file_in(&dir)? {
                    crate::add::replace_binding_uri(&path, key, &uri)
                        .map_err(|e| Error::install(e.to_string()))?;
                }
            }
            lookup::upsert_pin(&mut accumulated, key.clone(), spec_at_slot(&spec, &slot));
            slots.push(slot);
        }
        let lock_path = lock::path_in(&dir);
        lock::write_tools(&lock_path, &slots).map_err(|e| Error::install(e.to_string()))?;
    }
    if mode == PinMode::Update {
        print_update(&bumped);
    }
    Ok(())
}

fn spec_at_slot(spec: &Spec, slot: &lock::LockSlot) -> Spec {
    Spec {
        prefix: spec.prefix.clone(),
        package: spec.package.clone(),
        options: spec.options.clone(),
        version: slot.version.clone(),
        uri: spec::replace_version(&spec.uri, &slot.version),
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
    Ok(Spec {
        prefix: spec.prefix.clone(),
        package: spec.package.clone(),
        options: spec.options.clone(),
        version: version.clone(),
        uri: spec::replace_version(&spec.uri, &version),
    })
}

fn print_update(bumped: &[(String, String, String)]) {
    let mut mb = crate::message_box::MessageBox::new().info();
    if bumped.is_empty() {
        mb = mb.line("Lock already at the latest matching bins.");
    } else {
        mb = mb.line("Updated the lock and installed the new bins.");
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
