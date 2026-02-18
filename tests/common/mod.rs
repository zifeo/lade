use assert_cmd::Command;

pub fn lade(home: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("lade").unwrap();
    cmd.env("LADE_SHELL", "bash")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"));
    cmd
}

#[cfg(unix)]
pub fn fake_cli(dir: &tempfile::TempDir, name: &str, script_body: &str) {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let path = dir.path().join(name);
    fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}
