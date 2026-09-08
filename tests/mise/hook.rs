use super::common;
use super::support::*;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn inject_mise_intercept_removes_temp_config_after_run() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let bin = dir.path().join("bin");
    let tickets = home.path().join("tickets");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&tickets).unwrap();
    write_exec(&bin.join("mise"), "exit 0");
    fs::write(
        dir.path().join("lade.yml"),
        "^mise:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", &tickets)
        .env("PATH", prepend_path(&bin))
        .args(["inject", "--", "mise", "ls"])
        .assert()
        .success();
    let leftovers: Vec<_> = tickets
        .read_dir()
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("lade-mise-"))
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[cfg(unix)]
#[test]
fn hook_inject_runs_pinned_cargo() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tmp = tempdir().unwrap();
    let installs = dir.path().join("installs");
    fs::create_dir_all(installs.join("rust/1.96.0")).unwrap();
    write_exec(&installs.join("rust/1.96.0/cargo"), "echo PINNED");
    write_cached_env(
        home.path(),
        "core:rust",
        "core-rust",
        "1.96.0",
        r#"{"RUSTUP_TOOLCHAIN":"1.96.0"}"#,
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    let payload = r#"{"tool_name":"Shell","tool_input":{"command":"cargo test"},"hook_event_name":"preToolUse","conversation_id":"conv_1"}"#;
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .env("CURSOR_VERSION", "1.0")
        .env("MISE_INSTALLS_DIR", &installs)
        .args(["hook", "--harness", "cursor"])
        .write_stdin(payload)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let command = parsed["updated_input"]["command"]
        .as_str()
        .or_else(|| parsed["hookSpecificOutput"]["updatedInput"]["command"].as_str())
        .expect("wrap command");
    assert!(command.contains("--pretool="), "{command}");
    let id_start = command.find("--pretool=").expect("pretool") + "--pretool=".len();
    let id: String = command[id_start..].chars().take(4).collect();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .args([format!("--pretool={id}"), "cargo".into(), "test".into()])
        .assert()
        .success()
        .stdout(predicates::str::contains("PINNED"));
}
