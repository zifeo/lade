use assert_cmd::Command;

pub fn lade(home: &std::path::Path) -> Command {
    let config_path = home.join("lade-config.json");
    if !config_path.exists() {
        std::fs::write(
            &config_path,
            format!(
                r#"{{"update_check":"2099-01-01T00:00:00Z","self_version":"{}","user":null,"cli_check":{{}}}}"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .unwrap();
    }
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("lade"));
    cmd.env("LADE_SHELL", "bash")
        .env("HOME", home)
        .env("LADE_CONFIG_PATH", config_path)
        .env_remove("LADE_VIA")
        .env_remove("AI_AGENT")
        .env_remove("AGENT")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE")
        .env_remove("CURSOR_AGENT")
        .env_remove("COPILOT_MODEL")
        .env_remove("CURSOR_VERSION")
        .env_remove("CURSOR_EXTENSION_HOST_ROLE")
        .env_remove("CURSOR_SANDBOX")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SANDBOX")
        .env_remove("CODEX_CI")
        .env_remove("PI_MODEL")
        .env_remove("PI_SESSION_ID")
        .env_remove("OPENCODE")
        .env_remove("OPENCODE_PID");
    cmd
}
