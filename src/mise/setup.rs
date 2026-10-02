use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::decide::{Choice, decide, with_spec};
use super::ensure;
use super::error::Error;
use super::implied;
use super::install;
use super::lock;
use super::lookup;
use super::plane::{self, Plane, Snapshot};
use super::project;
use super::spec::{self, Spec};
use super::store;
use super::toml_merge;
use crate::live_progress::named_version;

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
    let installs = store::installs_dir();
    let mut dirs = snap.yaml_dirs();
    dirs.reverse();
    let mut pending: Vec<(String, Spec)> = Vec::new();
    let mut yaml_keys = HashSet::new();
    let mut key_dir: HashMap<String, PathBuf> = HashMap::new();
    for dir in dirs {
        for (key, value) in config.pins_in_dir(&dir, &saved) {
            let spec = spec::parse(&value).map_err(Error::invalid_spec)?;
            lookup::upsert_pin(&mut pending, key.clone(), spec);
            yaml_keys.insert(key.clone());
            key_dir.insert(key, dir.clone());
        }
        for (key, spec) in implied::pins_for(&config.sources_in_dir(&dir, &saved), &pending) {
            lookup::upsert_pin(&mut pending, key.clone(), spec);
            key_dir.entry(key).or_insert_with(|| dir.clone());
        }
    }
    if pending.is_empty() {
        warn_plane(&snap);
        return Ok(PinReport {
            mise: ensure::path_status().await.version,
            tools: Vec::new(),
            bumped: Vec::new(),
        });
    }
    let mut bumped = Vec::new();
    let mut works: Vec<(String, Choice)> = Vec::new();
    let lock_path = snap.lock_path().map(Path::to_path_buf);
    for (key, spec) in pending {
        let choice = decide(&snap, &key, spec.clone(), yaml_keys.contains(&key));
        let names = store::tool_names(choice.spec(), &key, None);
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let existing = lock_path.as_ref().and_then(|path| {
            path.is_file()
                .then(|| lock::slot_for(path, &name_refs).map(|slot| (path.clone(), slot)))
                .flatten()
        });
        let choice = match mode {
            PinMode::Update => {
                let old = existing
                    .as_ref()
                    .map(|(_, slot)| slot.version.as_str())
                    .unwrap_or("");
                let mut next = resolve_for_update(&key, &spec, old)?;
                next.install_id.clone_from(&choice.spec().install_id);
                next.options.clone_from(&choice.spec().options);
                if old != next.version {
                    bumped.push((key.clone(), old.to_string(), next.version.clone()));
                }
                with_spec(choice, next)
            }
            PinMode::Unlock | PinMode::Locked => choice,
        };
        let spec = choice.spec();
        let lock_ok = mode != PinMode::Unlock
            && existing
                .as_ref()
                .is_some_and(|(_, slot)| lock::agrees(slot, &spec.version, &spec.backend_id()));
        let mut spec = if lock_ok && let Some((_, slot)) = existing.as_ref() {
            spec::at_version(spec, &slot.version)
        } else if spec.is_range() {
            let label = named_version(&key, &spec.version);
            crate::live_progress::running(&key, &label);
            let version = match super::resolve::concrete_version(spec) {
                Ok(version) => version,
                Err(e) => {
                    crate::live_progress::failed(&key, &label);
                    return Err(e.into());
                }
            };
            rewrite_floating_pin(&key_dir, &key, spec, &version)?;
            crate::live_progress::done(&key, named_version(&key, &version));
            spec::at_version(spec, &version)
        } else {
            spec.clone()
        };
        let choice = if snap.is_mise() && matches!(&choice, Choice::Builtin(_)) {
            spec.options.clear();
            Choice::Builtin(spec)
        } else {
            with_spec(choice, spec)
        };
        works.push((key, choice));
    }
    warn_plane(&snap);
    let pins: Vec<(String, Spec)> = works
        .iter()
        .map(|(key, choice)| (key.clone(), choice.spec().clone()))
        .collect();
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
        if let Plane::Mise { toml_write, .. } = &snap.plane {
            for (_, choice) in &works {
                if let Choice::Builtin(spec) = choice {
                    install::use_in_project(spec, toml_write, &installs).await?;
                }
            }
        }
        install_group(&pins, &lock_path, &theirs, &installs, &cwd, upgrade).await?;
        if let Plane::Mise { toml_write, .. } = &snap.plane {
            let owned: Vec<(String, Spec)> = works
                .iter()
                .filter(|(_, choice)| {
                    matches!(
                        (mode, choice),
                        (_, Choice::Pin(_)) | (PinMode::Update, Choice::Row(_))
                    )
                })
                .map(|(key, choice)| (key.clone(), choice.spec().clone()))
                .collect();
            if !owned.is_empty() {
                let stale: Vec<String> = works
                    .iter()
                    .filter(|(_, choice)| matches!(choice, Choice::Pin(_)))
                    .filter(|(key, choice)| *key != project::tool_key(key, choice.spec()))
                    .map(|(key, _)| key.clone())
                    .collect();
                toml_merge::remove_tool_keys(toml_write, &stale).map_err(Error::install)?;
                let entries: Vec<(String, String)> = owned
                    .iter()
                    .map(|(key, spec)| (project::tool_key(key, spec), spec.version.clone()))
                    .collect();
                toml_merge::upsert_tools(toml_write, &entries).map_err(Error::install)?;
            }
        }
    } else {
        for (key, spec) in &pins {
            let label = named_version(key, &spec.version);
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
        tools: pins
            .iter()
            .map(|(key, spec)| (key.clone(), spec.version.clone()))
            .collect(),
        bumped,
    })
}

async fn install_group(
    pins: &[(String, Spec)],
    lock_path: &Path,
    theirs: &[project::ProjectTool],
    installs: &Path,
    cwd: &Path,
    upgrade: bool,
) -> Result<(), Error> {
    if pins.is_empty() {
        return Ok(());
    }
    let rewrite_lock = upgrade || !lock_matches_pins(lock_path, pins);
    if rewrite_lock {
        let label = if upgrade {
            "mise lock --upgrade"
        } else {
            "mise lock"
        };
        crate::live_progress::running("lock", label);
        if let Err(e) = install::refresh_lock(
            &project::compose_toml(theirs, pins),
            lock_path,
            installs,
            cwd,
            upgrade,
        )
        .await
        {
            crate::live_progress::failed("lock", label);
            return Err(e);
        }
        crate::live_progress::done("lock", label);
    }
    for (key, spec) in pins {
        let label = named_version(key, &spec.version);
        crate::live_progress::running(key, &label);
        if let Err(e) = install::install_locked(lock_path, spec, installs, cwd).await {
            crate::live_progress::failed(key, &label);
            return Err(e);
        }
        crate::live_progress::done(key, &label);
    }
    Ok(())
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

fn resolve_for_update(key: &str, spec: &Spec, current: &str) -> Result<Spec, Error> {
    let implied = implied::by_key(key).is_some();
    if !implied && !spec.is_range() {
        return Ok(spec.clone());
    }
    let label = named_version(key, &spec.version);
    crate::live_progress::running(key, &label);
    let version = match super::resolve::concrete_for_update(spec, current) {
        Ok(version) => version,
        Err(e) => {
            crate::live_progress::failed(key, &label);
            return Err(e);
        }
    };
    crate::live_progress::done(key, named_version(key, &version));
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
