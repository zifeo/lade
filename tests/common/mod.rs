use assert_cmd::Command;

pub fn lade(home: &std::path::Path) -> Command {
    let config_path = home.join("lade-config.json");
    if !config_path.exists() {
        // Far-future stamp: tests must not hit GitHub on `set` / `status`.
        std::fs::write(
            &config_path,
            r#"{"update_check":"2099-01-01T00:00:00Z","user":null,"cli_check":{}}"#,
        )
        .unwrap();
    }
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("lade"));
    cmd.env("LADE_SHELL", "bash")
        .env("HOME", home)
        // `directories` uses OS APIs (XDG on Linux, SHGetKnownFolderPath on Windows)
        // that ignore env vars, so we use a dedicated override instead.
        .env("LADE_CONFIG_PATH", config_path);
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
