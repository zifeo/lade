use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::ensure;
use super::error::Error;
use super::implied;
use super::install;
use super::lock::{self, LockSlot};
use super::lookup;
use super::plane::{self, Plane, Snapshot};
use super::project;
use super::spec::{self, Spec};
use super::store;
use super::toml_merge;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinMode {
    Locked,
    Unlock,
    Update,
}

#[derive(Clone, Debug, Default)]
pub struct PinReport {
    pub mise: Option<String>,
    pub tools: Vec<(String, String)>,
    pub bumped: Vec<(String, String, String)>,
}

impl PinReport {
    pub fn is_empty(&self) -> bool {
        self.mise.is_none() && self.tools.is_empty()
    }
}

pub async fn setup_pins(mode: PinMode) -> anyhow::Result<PinReport> {
    let cwd = std::env::current_dir().map_err(|e| Error::install(e.to_string()))?;
    let config = crate::config::LadeFile::build(cwd.clone())?;
    let saved = crate::global_config::GlobalConfig::user_from_disk();
    if !implied::repo_needs_mise(&config, &saved) {
        return Ok(PinReport::default());
    }
    crate::live_progress::running("mise", "mise");
    ensure::ensure_for_setup().await.inspect_err(|e| {
        crate::live_progress::failed("mise", "mise");
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
        return Ok(PinReport {
            mise: ensure::path_status().await.version,
            tools: Vec::new(),
            bumped: Vec::new(),
        });
    }
    let mut bumped = Vec::new();
    let lock_path = snap.lock_path().map(Path::to_path_buf);
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
            PinMode::Locked => heal_from_toml(&snap, key, spec),
        };
        let lock_ok = mode != PinMode::Unlock
            && existing.as_ref().is_some_and(|(_, slot)| {
                lock::agrees(slot, &spec.version, &spec.backend_id())
                    && (spec.is_range()
                        || spec::version_is_floating(&spec.version)
                        || spec.version == slot.version)
            });
        let spec = if lock_ok && let Some((_, slot)) = existing.as_ref() {
            spec::at_version(&spec, &slot.version)
        } else if spec.is_range() {
            let label = pin_label(key, &spec.version);
            crate::live_progress::running(key, &label);
            match super::latest_matching(&spec) {
                Ok(version) if !version.is_empty() => {
                    rewrite_floating_pin(&key_dir, key, &spec, &version)?;
                    crate::live_progress::done(key, pin_label(key, &version));
                    spec::at_version(&spec, &version)
                }
                _ => {
                    if let Err(e) = install::install_from_url(&spec, &installs, &cwd).await {
                        crate::live_progress::failed(key, &label);
                        return Err(e.into());
                    }
                    let slot = lookup::slot_after_install(key, &spec, &installs);
                    rewrite_floating_pin(&key_dir, key, &spec, &slot.version)?;
                    crate::live_progress::done(key, pin_label(key, &slot.version));
                    spec::at_version(&spec, &slot.version)
                }
            }
        } else {
            spec.clone()
        };
        lookup::upsert_pin(&mut accumulated, key.clone(), spec);
    }
    if let Some(lock_path) = lock_path {
        let theirs = match &snap.plane {
            Plane::Mise {
                toml, toml_write, ..
            } => {
                let path = toml.as_ref().unwrap_or(toml_write);
                project::project_tools(path)
            }
            Plane::Lade { .. } | Plane::None => Vec::new(),
        };
        let upgrade = mode == PinMode::Update;
        let rewrite_lock = upgrade || !lock_matches_pins(&lock_path, &accumulated);
        if rewrite_lock {
            let label = if upgrade {
                "mise lock --upgrade"
            } else {
                "mise lock"
            };
            crate::live_progress::running("lock", label);
            if let Err(e) = install::refresh_lock(
                &project::compose_toml(&theirs, &accumulated),
                &lock_path,
                &installs,
                &cwd,
                upgrade,
            )
            .await
            {
                crate::live_progress::failed("lock", label);
                return Err(e.into());
            }
            crate::live_progress::done("lock", label);
        }
        for (key, spec) in &accumulated {
            let label = pin_label(key, &spec.version);
            crate::live_progress::running(key, &label);
            if let Err(e) = install::install_locked(&lock_path, spec, &installs, &cwd).await {
                crate::live_progress::failed(key, &label);
                return Err(e.into());
            }
            crate::live_progress::done(key, &label);
        }
        if let Plane::Mise { toml_write, .. } = &snap.plane {
            let entries: Vec<(String, String)> = accumulated
                .iter()
                .map(|(key, spec)| (key.clone(), spec.version.clone()))
                .collect();
            toml_merge::upsert_tools(toml_write, &entries).map_err(Error::install)?;
        }
    } else {
        for (key, spec) in &accumulated {
            let label = pin_label(key, &spec.version);
            crate::live_progress::running(key, &label);
            if let Err(e) = install::install_from_url(spec, &installs, &cwd).await {
                crate::live_progress::failed(key, &label);
                return Err(e.into());
            }
            crate::live_progress::done(key, &label);
        }
    }
    Ok(PinReport {
        mise: ensure::path_status().await.version,
        tools: accumulated
            .iter()
            .map(|(key, spec)| (key.clone(), spec.version.clone()))
            .collect(),
        bumped,
    })
}

fn pin_label(key: &str, version: &str) -> String {
    crate::live_progress::named_version(key, version)
}

fn rewrite_floating_pin(
    key_dir: &HashMap<String, PathBuf>,
    key: &str,
    spec: &Spec,
    resolved: &str,
) -> Result<(), Error> {
    if !spec::version_is_floating(&spec.version) || resolved == spec.version {
        return Ok(());
    }
    let uri = spec::replace_version(&spec.uri, resolved);
    if let Some(dir) = key_dir.get(key)
        && let Some(path) = lookup::yaml_file_in(dir)?
    {
        crate::command::add::replace_binding_uri(&path, key, &uri)
            .map_err(|e| Error::install(e.to_string()))?;
    }
    Ok(())
}

fn heal_from_toml(snap: &Snapshot, key: &str, spec: &Spec) -> Spec {
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
    spec::at_version(spec, &version)
}

fn lock_matches_pins(lock_path: &Path, pins: &[(String, Spec)]) -> bool {
    if !lock_path.is_file() {
        return false;
    }
    pins.iter().all(|(key, spec)| {
        let names = store::tool_names(spec, key, None);
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        lock::slot_for(lock_path, &name_refs).is_some_and(|slot| slot.version == spec.version)
    })
}

fn resolve_for_update(key: &str, spec: &Spec) -> Result<Spec, Error> {
    let implied = implied::by_key(key).is_some();
    if !implied && !spec.is_range() {
        return Ok(spec.clone());
    }
    crate::live_progress::running(key, pin_label(key, &spec.version));
    let Ok(version) = super::latest_matching(spec) else {
        return Ok(spec.clone());
    };
    if version.is_empty() {
        return Ok(spec.clone());
    }
    crate::live_progress::done(key, pin_label(key, &version));
    Ok(spec::at_version(spec, &version))
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

pub fn print_pin_update(bumped: &[(String, String, String)]) {
    let mut report = crate::message_box::Report::new();
    if bumped.is_empty() {
        report = report
            .line("Lock already at the latest matching packages.")
            .dim("Exact pins stay. Implied and ranged pins already match.");
    } else {
        report = report
            .heading("Updated the lock and installed the new packages.")
            .dim("Exact pins stay. Implied and ranged pins moved.");
        for (key, from, to) in bumped {
            if from.is_empty() {
                report = report.line(format!("  {key} {to}"));
            } else {
                report = report.line(format!("  {key} {from} -> {to}"));
            }
        }
    }
    report.print();
}
