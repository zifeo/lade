use std::path::{Path, PathBuf};

/// The file `PATH` gives for `program`, as the child would find it. A
/// relative entry before the match leaves the search to `exec`.
pub(super) fn resolve(program: &str, path: &str) -> Option<PathBuf> {
    for dir in path.split(':') {
        if !dir.starts_with('/') {
            return None;
        }
        let candidate = Path::new(dir).join(program);
        if candidate.is_file()
            && nix::unistd::access(&candidate, nix::unistd::AccessFlags::X_OK).is_ok()
        {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_follows_path_order_and_stops_at_a_relative_entry() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let tool = |at: &Path, mode: u32| {
            use std::os::unix::fs::PermissionsExt;
            let file = at.join("tool");
            std::fs::write(&file, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
            file
        };
        tool(&first, 0o644);
        let runs = tool(&second, 0o755);
        let path = format!("{}:{}", first.display(), second.display());
        assert_eq!(resolve("tool", &path), Some(runs));
        assert_eq!(resolve("tool", &format!("bin:{path}")), None);
        assert_eq!(resolve("missing", &path), None);
    }
}
