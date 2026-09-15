use super::ensure;
use super::error::Error;
use super::implied;
use super::install;
use super::lock;
use super::lookup;
use super::spec;
use super::store;

pub async fn setup_pins() -> anyhow::Result<()> {
    let cwd = std::env::current_dir().map_err(|e| Error::install(e.to_string()))?;
    let config = crate::config::LadeFile::build(cwd.clone())?;
    let saved = crate::global_config::GlobalConfig::user_from_disk();
    if !implied::repo_needs_mise(&config, &saved) {
        return Ok(());
    }
    ensure::ensure_for_setup().await.inspect_err(|e| {
        e.emit();
    })?;
    let installs = store::installs_dir();
    let mut dirs = lookup::yaml_dirs(&cwd)?;
    dirs.reverse();
    let mut accumulated: Vec<(String, spec::Spec)> = Vec::new();
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
        for (key, spec) in &accumulated {
            let names = store::tool_names(spec, key, None);
            let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
            if let Some((path, slot)) = lock::slot_and_path(&dir, &name_refs)
                && lock::agrees(&slot, &spec.version, &spec.backend_id())
            {
                install::install_locked(&slot.name, &path, spec, &installs, &cwd).await?;
                continue;
            }
            install::install_from_url(spec, &installs, &cwd).await?;
        }
        let slots: Vec<lock::LockSlot> = accumulated
            .iter()
            .map(|(key, spec)| lookup::slot_after_install(key, spec, &installs))
            .collect();
        for ((key, spec), slot) in accumulated.iter().zip(slots.iter()) {
            if spec::version_is_floating(&spec.version) && slot.version != spec.version {
                let uri = spec::replace_version(&spec.uri, &slot.version);
                if let Some(path) = lookup::yaml_file_in(&dir)? {
                    crate::add::replace_binding_uri(&path, key, &uri)
                        .map_err(|e| Error::install(e.to_string()))?;
                }
            }
        }
        let lock_path = dir.join("lade.lock");
        lock::write_tools(&lock_path, &slots).map_err(|e| Error::install(e.to_string()))?;
    }
    Ok(())
}
