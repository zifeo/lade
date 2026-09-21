use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::message_box::Report;

const QUALIFIER: &str = "com";
const ORG: &str = "zifeo";
const APP: &str = "lade";

pub fn root() -> PathBuf {
    if let Ok(path) = std::env::var("LADE_CACHE_DIR")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    directories::ProjectDirs::from(QUALIFIER, ORG, APP)
        .map(|project| project.cache_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".lade-cache"))
}

pub fn mise_env() -> PathBuf {
    root().join("mise-env")
}

pub fn mise_shims() -> PathBuf {
    root().join("mise-shims")
}

pub fn tickets() -> PathBuf {
    root().join("tickets")
}

pub fn network_logs() -> PathBuf {
    root().join("network")
}

pub fn scratch() -> PathBuf {
    root().join("tmp")
}

pub fn scratch_tempdir() -> io::Result<tempfile::TempDir> {
    fs::create_dir_all(scratch())?;
    tempfile::Builder::new()
        .prefix("lade-")
        .tempdir_in(scratch())
}

pub fn mise_isolate(cwd: &Path) -> PathBuf {
    root().join("mise").join(path_key(cwd))
}

/// Pin-only mise plane for `cwd`. Rewrite `mise.toml` only when bytes change.
/// `lock_src` is the repo lock; `mise.lock` in the isolate dir is a symlink to it.
pub fn prepare_mise_project(
    cwd: &Path,
    toml: &str,
    lock_src: Option<&Path>,
) -> io::Result<PathBuf> {
    let dir = mise_isolate(cwd);
    fs::create_dir_all(&dir)?;
    write_if_changed(&dir.join("mise.toml"), toml.as_bytes())?;
    if let Some(src) = lock_src {
        if let Some(parent) = src.parent() {
            fs::create_dir_all(parent)?;
        }
        replace_link(&dir.join("mise.lock"), src)?;
    }
    Ok(dir)
}

pub fn write_if_changed(path: &Path, body: &[u8]) -> io::Result<bool> {
    if path.is_file()
        && let Ok(existing) = fs::read(path)
        && existing == body
    {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(path, body)?;
    Ok(true)
}

/// After `mise lock` in the isolate dir, keep the repo lock as the real file
/// and the isolate `mise.lock` as an alias.
pub fn publish_lock(isolate_lock: &Path, dest: &Path) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let dest_abs = absolute(dest)?;
    if link_points_at(isolate_lock, &dest_abs) {
        if dest_abs.is_file() {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "mise lock did not write a lockfile",
        ));
    }
    if !isolate_lock.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "mise lock did not write a lockfile",
        ));
    }
    fs::copy(isolate_lock, &dest_abs)?;
    let _ = fs::remove_file(isolate_lock);
    replace_link(isolate_lock, &dest_abs)
}

pub fn is_persistent_mise_config(path: &Path) -> bool {
    let root = root().join("mise");
    let candidate = absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if candidate.starts_with(&root) {
        return true;
    }
    match (fs::canonicalize(&candidate), fs::canonicalize(&root)) {
        (Ok(candidate), Ok(root)) => candidate.starts_with(root),
        _ => false,
    }
}

pub fn clear_and_report() -> anyhow::Result<()> {
    let cache = root();
    let had_cache = cache.exists();
    if had_cache {
        fs::remove_dir_all(&cache)?;
    }
    let legacy_tickets = std::env::temp_dir().join("lade-t");
    let had_legacy = legacy_tickets.exists();
    if had_legacy {
        let _ = fs::remove_dir_all(&legacy_tickets);
    }
    remove_legacy_os_temp_logs();
    let mut report = Report::new();
    if had_cache || had_legacy {
        report = report.heading("Cleared this machine's Lade cache.");
    } else {
        report = report.heading("Lade cache was already empty.");
    }
    report
        .line(format!("  {}", cache.display()))
        .blank()
        .dim("Home pre-tool hooks stay.")
        .dim("`lade hook disable --scope user` removes them.")
        .print();
    Ok(())
}

fn path_key(path: &Path) -> String {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    hex_prefix(&digest, 8)
}

fn hex_prefix(bytes: &[u8], n: usize) -> String {
    let mut out = String::with_capacity(n * 2);
    for b in bytes.iter().take(n) {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn atomic_write(path: &Path, body: &[u8]) -> io::Result<()> {
    let tmp = match path.file_name() {
        Some(name) => path.with_file_name(format!("{}.tmp", name.to_string_lossy())),
        None => path.with_extension("tmp"),
    };
    fs::write(&tmp, body)?;
    fs::rename(&tmp, path)
}

fn replace_link(link: &Path, target: &Path) -> io::Result<()> {
    let target = absolute(target)?;
    if link_points_at(link, &target) {
        return Ok(());
    }
    let _ = fs::remove_file(link);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, link)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        if target.is_file() {
            fs::copy(&target, link)?;
        }
        Ok(())
    }
}

fn link_points_at(link: &Path, target: &Path) -> bool {
    let Ok(current) = fs::read_link(link) else {
        return false;
    };
    let current = if current.is_absolute() {
        current
    } else {
        match link.parent() {
            Some(parent) => parent.join(current),
            None => current,
        }
    };
    current == target
}

fn absolute(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn remove_legacy_os_temp_logs() {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with("lade-network-") && name.ends_with(".log") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn with_cache(f: impl FnOnce(&Path)) {
        let dir = tempdir().unwrap();
        temp_env::with_var("LADE_CACHE_DIR", Some(dir.path()), || f(dir.path()));
    }

    #[test]
    fn write_if_changed_skips_identical_bytes() {
        with_cache(|cache| {
            let path = cache.join("mise.toml");
            assert!(write_if_changed(&path, b"a = 1\n").unwrap());
            assert!(!write_if_changed(&path, b"a = 1\n").unwrap());
            assert!(write_if_changed(&path, b"a = 2\n").unwrap());
            assert_eq!(fs::read_to_string(&path).unwrap(), "a = 2\n");
        });
    }

    #[cfg(unix)]
    #[test]
    fn prepare_aliases_lock_and_reuses_toml() {
        with_cache(|cache| {
            let repo = tempdir().unwrap();
            let dest = repo.path().join("lade.lock");
            fs::write(&dest, "v1\n").unwrap();
            let root = prepare_mise_project(repo.path(), "[tools]\njq = \"1.7.1\"\n", Some(&dest))
                .unwrap();
            assert!(root.starts_with(cache.join("mise")));
            let toml = root.join("mise.toml");
            let first = fs::metadata(&toml).unwrap().modified().unwrap();
            prepare_mise_project(repo.path(), "[tools]\njq = \"1.7.1\"\n", Some(&dest)).unwrap();
            let second = fs::metadata(&toml).unwrap().modified().unwrap();
            assert_eq!(first, second);
            let link = root.join("mise.lock");
            assert_eq!(fs::read_link(&link).unwrap(), dest);
            assert_eq!(fs::read_to_string(&link).unwrap(), "v1\n");
        });
    }

    #[cfg(unix)]
    #[test]
    fn publish_lock_copies_then_aliases() {
        with_cache(|_| {
            let repo = tempdir().unwrap();
            let isolate = tempdir().unwrap();
            let dest = repo.path().join("lade.lock");
            let written = isolate.path().join("mise.lock");
            fs::write(&written, "locked\n").unwrap();
            publish_lock(&written, &dest).unwrap();
            assert_eq!(fs::read_to_string(&dest).unwrap(), "locked\n");
            assert_eq!(fs::read_link(&written).unwrap(), dest);
        });
    }

    #[test]
    fn persistent_config_is_under_mise_isolate() {
        with_cache(|_| {
            let repo = tempdir().unwrap();
            let root = prepare_mise_project(repo.path(), "[tools]\n", None).unwrap();
            assert!(is_persistent_mise_config(&root.join("mise.toml")));
            assert!(!is_persistent_mise_config(&repo.path().join("mise.toml")));
        });
    }

    #[test]
    fn clear_removes_cache_tree() {
        with_cache(|cache| {
            let marker = cache.join("tickets").join("ab12.json");
            fs::create_dir_all(marker.parent().unwrap()).unwrap();
            fs::write(&marker, "{}").unwrap();
            fs::remove_dir_all(cache).unwrap();
            assert!(!cache.exists());
        });
    }
}
