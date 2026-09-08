use std::fs;
use std::io::{self, Error, ErrorKind};
use std::path::{Path, PathBuf};

pub const PLUGIN_BIN: &str = "age-plugin-lade";

pub fn is_plugin_argv0(argv0: &std::ffi::OsStr) -> bool {
    let name = Path::new(argv0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    name.eq_ignore_ascii_case(PLUGIN_BIN)
}

/// `lade upgrade` only. Install and `cargo install` already ship both bins.
/// Copy into a sibling first so a failed copy leaves the existing plugin.
pub fn copy_beside(lade_path: &Path) -> io::Result<PathBuf> {
    let dir = lade_path
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "lade path has no parent directory"))?;
    let dest = dir.join(PLUGIN_BIN);
    let tmp = dir.join(format!("{PLUGIN_BIN}.new"));
    if tmp.exists() {
        fs::remove_file(&tmp)?;
    }
    fs::copy(lade_path, &tmp)?;
    if let Err(err) = fs::rename(&tmp, &dest) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(dest)
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
    fn copy_beside_writes_a_second_file() {
        let dir = tempfile::tempdir().unwrap();
        let lade = dir.path().join("lade");
        fs::write(&lade, b"bin").unwrap();
        let dest = copy_beside(&lade).unwrap();
        assert_eq!(dest.file_name().unwrap(), PLUGIN_BIN);
        assert_eq!(fs::read(&dest).unwrap(), b"bin");
        assert!(
            !fs::symlink_metadata(&dest)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let dest2 = copy_beside(&lade).unwrap();
        assert_eq!(dest, dest2);
    }

    #[test]
    fn copy_beside_keeps_dest_when_source_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join(PLUGIN_BIN);
        fs::write(&dest, b"old").unwrap();
        let missing = dir.path().join("lade");
        assert!(copy_beside(&missing).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"old");
    }
}
