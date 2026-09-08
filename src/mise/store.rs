use std::path::{Path, PathBuf};

use super::spec::Spec;

pub fn installs_dir() -> PathBuf {
    if let Ok(path) = std::env::var("MISE_INSTALLS_DIR")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("MISE_DATA_DIR")
        && !path.is_empty()
    {
        return PathBuf::from(path).join("installs");
    }
    directories::UserDirs::new()
        .map(|user| user.home_dir().join(".local/share/mise/installs"))
        .unwrap_or_else(|| PathBuf::from(".local/share/mise/installs"))
}

pub fn tool_names(spec: &Spec, pin_key: &str, lock_name: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    for name in [
        lock_name.map(str::to_string),
        Some(spec.short_name().to_string()),
        Some(pin_key.to_string()),
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
    [root.join("bin"), root.clone(), root.join(".mise-bins")]
        .into_iter()
        .find(|dir| bin_exists(dir, argv0))
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
            },
        );
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
}
