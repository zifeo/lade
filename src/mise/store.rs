use std::path::{Path, PathBuf};

use super::spec::Spec;

pub fn data_dir() -> PathBuf {
    if let Ok(path) = std::env::var("MISE_DATA_DIR")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    directories::UserDirs::new()
        .map(|user| user.home_dir().join(".local/share/mise"))
        .unwrap_or_else(|| PathBuf::from(".local/share/mise"))
}

pub fn installs_dir() -> PathBuf {
    if let Ok(path) = std::env::var("MISE_INSTALLS_DIR")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    data_dir().join("installs")
}

pub fn tool_names(spec: &Spec, pin_key: &str, lock_name: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    for name in [
        lock_name.map(str::to_string),
        Some(spec.short_name().to_string()),
        Some(pin_key.to_string()),
        Some(spec.backend_id()),
        Some(spec.backend_slug()),
        Some(spec.backend_id().replace(['/', ':'], "-")),
    ]
    .into_iter()
    .flatten()
    {
        if !name.is_empty() && !names.iter().any(|existing| existing == &name) {
            names.push(name);
        }
    }
    names
}

pub fn find_bin_dir(installs: &Path, tool: &str, version: &str, argv0: &str) -> Option<PathBuf> {
    let root = installs.join(tool).join(version);
    if !root.is_dir() {
        return None;
    }
    for dir in [root.join("bin"), root.clone(), root.join(".mise-bins")] {
        if bin_exists(&dir, argv0) {
            return Some(dir);
        }
    }
    find_bin_dir_walk(&root, argv0, 8)
}

fn find_bin_dir_walk(dir: &Path, argv0: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() || !kind.is_dir() {
            continue;
        }
        dirs.push(entry.path());
    }
    dirs.sort_by(|left, right| {
        let left_bin = left.file_name().and_then(|name| name.to_str()) == Some("bin");
        let right_bin = right.file_name().and_then(|name| name.to_str()) == Some("bin");
        right_bin.cmp(&left_bin).then_with(|| left.cmp(right))
    });
    for path in dirs {
        if bin_exists(&path, argv0) {
            return Some(path);
        }
        if let Some(found) = find_bin_dir_walk(&path, argv0, depth - 1) {
            return Some(found);
        }
    }
    None
}

fn bin_exists(dir: &Path, argv0: &str) -> bool {
    if is_executable(&dir.join(argv0)) {
        return true;
    }
    #[cfg(windows)]
    if is_executable(&dir.join(format!("{argv0}.exe"))) {
        return true;
    }
    false
}

pub fn find_pinned_bin(
    installs: &Path,
    names: &[String],
    version: &str,
    argv0: &str,
) -> Option<PathBuf> {
    names
        .iter()
        .find_map(|name| find_bin_dir(installs, name, version, argv0))
}

pub fn resolve_matching_version(
    installs: &Path,
    names: &[String],
    argv0: &str,
    requested: &str,
) -> Option<String> {
    let mut best: Option<String> = None;
    for name in names {
        let root = installs.join(name);
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let version = entry.file_name().to_string_lossy().into_owned();
            if find_bin_dir(installs, name, &version, argv0).is_none() {
                continue;
            }
            if !version_matches(&version, requested) {
                continue;
            }
            best = Some(newer_version(best, version));
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

fn version_matches(installed: &str, requested: &str) -> bool {
    if requested == "*" || installed == requested || super::spec::version_is_floating(requested) {
        return true;
    }
    if !super::spec::version_is_range(requested) {
        return false;
    }
    let Ok(req) = semver::VersionReq::parse(requested) else {
        return false;
    };
    let Ok(found) = semver::Version::parse(installed) else {
        return false;
    };
    req.matches(&found)
}

fn newer_version(current: Option<String>, candidate: String) -> String {
    let Some(current) = current else {
        return candidate;
    };
    match (
        semver::Version::parse(&current),
        semver::Version::parse(&candidate),
    ) {
        (Ok(left), Ok(right)) if right > left => candidate,
        (Ok(_), Ok(_)) => current,
        _ if candidate > current => candidate,
        _ => current,
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        unsafe { libc::access(c_path.as_ptr(), libc::X_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mise::spec::parse;
    use tempfile::tempdir;

    #[cfg(unix)]
    fn write_exec(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, "#!/bin/sh\necho ok\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn prefers_bin_then_flat() {
        let dir = tempdir().unwrap();
        let installs = dir.path();
        let bin = installs.join("opentofu/1.8.2/bin");
        std::fs::create_dir_all(&bin).unwrap();
        write_exec(&bin.join("tofu"));
        let found = find_bin_dir(installs, "opentofu", "1.8.2", "tofu").unwrap();
        assert_eq!(found, bin);

        let jq = installs.join("jq/1.7.1");
        std::fs::create_dir_all(&jq).unwrap();
        write_exec(&jq.join("jq"));
        let found = find_bin_dir(installs, "jq", "1.7.1", "jq").unwrap();
        assert_eq!(found, jq);
    }

    #[cfg(unix)]
    #[test]
    fn tries_backend_slug() {
        let dir = tempdir().unwrap();
        let spec = parse("mise://aqua/jqlang/jq@1.7.1").unwrap();
        let names = tool_names(&spec, "jq", None);
        assert!(names.contains(&"aqua-jqlang-jq".to_string()));
        let root = dir.path().join("aqua-jqlang-jq/1.7.1");
        std::fs::create_dir_all(&root).unwrap();
        write_exec(&root.join("jq"));
        let found = find_pinned_bin(dir.path(), &names, "1.7.1", "jq").unwrap();
        assert_eq!(found, root);
    }

    #[test]
    fn honors_mise_installs_dir() {
        let dir = tempdir().unwrap();
        temp_env::with_var("MISE_INSTALLS_DIR", Some(dir.path()), || {
            assert_eq!(installs_dir(), dir.path());
        });
    }

    #[test]
    fn honors_mise_data_dir_when_installs_dir_unset() {
        let dir = tempdir().unwrap();
        temp_env::with_vars(
            [
                ("MISE_INSTALLS_DIR", None),
                ("MISE_DATA_DIR", Some(dir.path())),
            ],
            || {
                assert_eq!(installs_dir(), dir.path().join("installs"));
                assert_eq!(data_dir(), dir.path());
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn finds_nested_pkg_payload_bin() {
        let dir = tempdir().unwrap();
        let bin = dir
            .path()
            .join("aqua-fish-shell-fish-shell/4.9.3/fish.pkg/Payload/usr/local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        write_exec(&bin.join("fish"));
        let found =
            find_bin_dir(dir.path(), "aqua-fish-shell-fish-shell", "4.9.3", "fish").unwrap();
        assert_eq!(found, bin);
    }

    #[cfg(unix)]
    #[test]
    fn finds_mise_bins_layout() {
        let dir = tempdir().unwrap();
        let bins = dir.path().join("jq/1.7.1/.mise-bins");
        std::fs::create_dir_all(&bins).unwrap();
        write_exec(&bins.join("jq"));
        let found = find_bin_dir(dir.path(), "jq", "1.7.1", "jq").unwrap();
        assert_eq!(found, bins);
    }

    #[cfg(unix)]
    #[test]
    fn resolve_picks_installed_version() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("jq/2.0.0");
        std::fs::create_dir_all(&root).unwrap();
        write_exec(&root.join("jq"));
        let version = resolve_matching_version(dir.path(), &["jq".to_string()], "jq", "*").unwrap();
        assert_eq!(version, "2.0.0");
    }

    #[cfg(unix)]
    #[test]
    fn matching_keeps_exact_pin_not_sibling() {
        let dir = tempdir().unwrap();
        for version in ["1.7.1", "1.8.0", "1.10.0"] {
            let root = dir.path().join("jq").join(version);
            std::fs::create_dir_all(&root).unwrap();
            write_exec(&root.join("jq"));
        }
        let names = vec!["jq".to_string()];
        assert_eq!(
            resolve_matching_version(dir.path(), &names, "jq", "1.7.1").as_deref(),
            Some("1.7.1")
        );
        assert_eq!(
            resolve_matching_version(dir.path(), &names, "jq", ">=1.8.0").as_deref(),
            Some("1.10.0")
        );
    }
}
