//! Unit tests run inside this process. One that runs with the developer's
//! real HOME, by forgetting a temp HOME or `LADE_*`, gets its Lade state
//! (cache, tickets, diary, config) in one temp dir instead of theirs.

use std::path::Path;
use std::sync::OnceLock;

pub(crate) fn dir() -> &'static Path {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        tempfile::Builder::new()
            .prefix("lade-unit-")
            .tempdir()
            .expect("unit test state dir")
    })
    .path()
}

/// The real home comes from the user database: a test that set its own
/// HOME keeps the paths it derives from it.
pub(crate) fn uses_real_home() -> bool {
    let real = nix::unistd::User::from_uid(nix::unistd::getuid())
        .expect("user database")
        .expect("current user")
        .dir;
    std::env::var_os("HOME").is_none_or(|home| Path::new(&home) == real)
}
