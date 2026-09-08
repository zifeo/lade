use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use super::common;

pub(super) fn write_yml(dir: &Path, body: &str) {
    fs::write(dir.join("lade.yml"), body).unwrap();
}

pub(super) fn hook_stdout(home: &Path, dir: &Path, tmp: &Path, command: &str) -> String {
    let payload = format!(
        r#"{{"tool_name":"Shell","tool_input":{{"command":"{command}"}},"hook_event_name":"preToolUse","conversation_id":"conv_1"}}"#
    );
    let out = common::lade(home)
        .current_dir(dir)
        .env("TMPDIR", tmp)
        .env("TMP", tmp)
        .env("TEMP", tmp)
        .env("CURSOR_VERSION", "1.0")
        .args(["hook", "--harness", "cursor"])
        .write_stdin(payload)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8_lossy(&out).into_owned()
}

pub(super) fn wrap_command(stdout: &str) -> String {
    let parsed: Value = serde_json::from_str(stdout).unwrap();
    parsed["updated_input"]["command"]
        .as_str()
        .or_else(|| parsed["hookSpecificOutput"]["updatedInput"]["command"].as_str())
        .unwrap_or_else(|| panic!("wrap command in hook stdout: {stdout}"))
        .to_string()
}

pub(super) fn ticket_id_from_wrap(command: &str) -> String {
    let marker = "--pretool=";
    let start = command
        .find(marker)
        .unwrap_or_else(|| panic!("--pretool= in {command}"))
        + marker.len();
    command[start..].chars().take(4).collect()
}

pub(super) fn ticket_path(tmp: &Path, id: &str) -> PathBuf {
    tmp.join("lade-t").join(format!("{id}.json"))
}

pub(super) fn ticket_ids(tmp: &Path) -> Vec<String> {
    let dir = tmp.join("lade-t");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let id = name.strip_suffix(".json")?;
            Some(id.to_string())
        })
        .collect();
    ids.sort();
    ids
}

pub(super) fn log_rows(home: &Path, dir: &Path) -> Value {
    let out = common::lade(home)
        .current_dir(dir)
        .args(["log", "--json", "--since", "1d"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}
