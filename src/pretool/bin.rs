use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Bin name for hook enable and match rewrites.
/// Bare `lade` stays `lade`. A path in argv[0] uses current_exe unless `lade`
/// is already on PATH (Cursor often execs the resolved absolute path).
pub(crate) fn invoked_lade_bin() -> String {
    invoked_lade_bin_from(
        env::args_os().next(),
        env::current_exe().ok(),
        lade_on_path(),
    )
}

pub(crate) fn invoked_lade_bin_from(
    argv0: Option<OsString>,
    current_exe: Option<PathBuf>,
    on_path: bool,
) -> String {
    let argv0 = argv0.unwrap_or_default();
    let path = Path::new(&argv0);
    if !argv0.is_empty() && path.file_name() == Some(path.as_os_str()) {
        return path.to_str().unwrap_or("lade").to_string();
    }
    if on_path {
        if let Some(name) = lade_basename(path) {
            return name.to_string();
        }
        if let Some(name) = current_exe.as_ref().and_then(|p| lade_basename(p)) {
            return name.to_string();
        }
        return "lade".to_string();
    }
    current_exe
        .and_then(|p| p.to_str().map(str::to_string))
        .or_else(|| argv0.to_str().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "lade".to_string())
}

fn lade_basename(path: &Path) -> Option<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| matches!(*name, "lade" | "lade.exe"))
}

pub(crate) fn lade_on_path() -> bool {
    bin_on_path("lade") || bin_on_path("lade.exe")
}

fn bin_on_path(name: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|dir| dir.join(name).is_file())
}
