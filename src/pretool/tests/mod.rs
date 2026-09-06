use crate::config::{Config, LadeFile};
use tempfile::{TempDir, tempdir};

mod already;
mod detect;
mod when;
mod wrap;

const AGENT_ENV: [(&str, Option<&str>); 9] = [
    ("CURSOR_VERSION", None),
    ("CLAUDE_PROJECT_DIR", None),
    ("CODEX_THREAD_ID", None),
    ("CODEX_SANDBOX", None),
    ("CODEX_HOME", None),
    ("PI_HOME", None),
    ("PI_CODING_AGENT", None),
    ("OPENCODE", None),
    ("OPENCODE_DIR", None),
];

fn with_ticket_tmpdir<F: FnOnce()>(f: F) {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    temp_env::with_var("LADE_TICKET_DIR", Some(path), f);
}

fn assert_wraps_with_ticket(result: &str, command: &str) {
    assert!(result.contains("--pretool"));
    assert!(result.contains(&format!("'{command}'")));
    let after_flag = result
        .split("--pretool")
        .nth(1)
        .expect("result should contain --pretool");
    let token = after_flag
        .strip_prefix('=')
        .unwrap_or(after_flag)
        .split_whitespace()
        .next()
        .expect("token after --pretool");
    assert!(
        crate::ticket::is_id(token),
        "expected 4-char ticket id after --pretool, got {:?}",
        token
    );
}

fn test_config(pattern: &str) -> (Config, TempDir) {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        format!("\"{}\":\n  KEY: val\n", pattern),
    )
    .unwrap();
    (LadeFile::build(dir.path().to_path_buf()).unwrap(), dir)
}

fn test_config_with_disclaimer(pattern: &str, disclaimer: &str) -> (Config, TempDir) {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"{}\":\n  \".\":\n    disclaimer: \"{}\"\n  KEY: val\n",
            pattern, disclaimer
        ),
    )
    .unwrap();
    (LadeFile::build(dir.path().to_path_buf()).unwrap(), dir)
}

fn with_cursor_env<F: FnOnce()>(f: F) {
    temp_env::with_vars(
        [
            ("CURSOR_VERSION", Some("1.0")),
            ("CLAUDE_PROJECT_DIR", None),
            ("CODEX_THREAD_ID", None),
            ("CODEX_SANDBOX", None),
            ("CODEX_HOME", None),
            ("PI_HOME", None),
            ("PI_CODING_AGENT", None),
            ("OPENCODE", None),
            ("OPENCODE_DIR", None),
        ],
        f,
    );
}
