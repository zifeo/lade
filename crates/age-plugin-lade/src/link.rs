use std::path::{Path, PathBuf};

pub const PLUGIN_BIN: &str = "age-plugin-lade";
pub const LADE_CLI: &str = "lade";

pub fn is_plugin_argv0(argv0: &std::ffi::OsStr) -> bool {
    let name = Path::new(argv0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    name.eq_ignore_ascii_case(PLUGIN_BIN)
}

pub fn find_lade() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("LADE_BIN") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "LADE_BIN is set but {} is not a file. Install lade.",
            path.display()
        ));
    }
    if let Some(sibling) = sibling_lade() {
        return Ok(sibling);
    }
    if let Some(on_path) = find_on_path(LADE_CLI) {
        return Ok(on_path);
    }
    Err("lade not found. Install lade and keep age-plugin-lade next to it, or set LADE_BIN.".into())
}

fn sibling_lade() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(LADE_CLI);
    candidate.is_file().then_some(candidate)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_argv0_matches_name() {
        assert!(is_plugin_argv0(std::ffi::OsStr::new("age-plugin-lade")));
        assert!(is_plugin_argv0(std::ffi::OsStr::new(
            "/usr/local/bin/age-plugin-lade"
        )));
        assert!(is_plugin_argv0(std::ffi::OsStr::new("AGE-PLUGIN-LADE")));
        assert!(!is_plugin_argv0(std::ffi::OsStr::new("lade")));
        assert!(!is_plugin_argv0(std::ffi::OsStr::new("age-plugin-op")));
        assert!(!is_plugin_argv0(std::ffi::OsStr::new(
            "age-plugin-lade.exe"
        )));
    }

    #[test]
    fn lade_bin_env_wins() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("lade");
        std::fs::write(&bin, "").unwrap();
        temp_env::with_var("LADE_BIN", Some(bin.as_os_str()), || {
            assert_eq!(find_lade().unwrap(), bin);
        });
    }

    #[test]
    fn lade_bin_env_missing_file_errors() {
        temp_env::with_var("LADE_BIN", Some("/no/such/lade-bin"), || {
            let err = find_lade().unwrap_err();
            assert!(err.contains("LADE_BIN"), "{err}");
        });
    }

    #[test]
    fn path_finds_lade_when_no_env() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("lade");
        std::fs::write(&bin, "").unwrap();
        let path = dir.path().to_str().expect("utf-8 temp dir");
        temp_env::with_vars([("LADE_BIN", None), ("PATH", Some(path))], || {
            assert_eq!(find_lade().unwrap(), bin);
        });
    }
}
