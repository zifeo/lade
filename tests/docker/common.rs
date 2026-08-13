use assert_cmd::Command;

pub fn lade(home: &std::path::Path) -> Command {
    let config_path = home.join("lade-config.json");
    if !config_path.exists() {
        std::fs::write(
            &config_path,
            r#"{"update_check":"2099-01-01T00:00:00Z","user":null,"cli_check":{}}"#,
        )
        .unwrap();
    }
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("lade"));
    cmd.env("LADE_SHELL", "bash")
        .env("HOME", home)
        .env("LADE_CONFIG_PATH", config_path);
    cmd
}
