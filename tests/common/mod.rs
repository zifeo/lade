use assert_cmd::Command;

pub fn lade(home: &std::path::Path) -> Command {
    #[allow(deprecated)]
    let mut cmd = Command::cargo_bin("lade").unwrap();
    cmd.env("LADE_SHELL", "bash")
        .env("HOME", home)
        // `directories` uses OS APIs (XDG on Linux, SHGetKnownFolderPath on Windows)
        // that ignore env vars, so we use a dedicated override instead.
        .env("LADE_CONFIG_PATH", home.join("lade-config.json"));
    cmd
}

#[cfg(unix)]
#[allow(dead_code)]
pub fn fake_cli(dir: &tempfile::TempDir, name: &str, script_body: &str) {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let path = dir.path().join(name);
    fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}
