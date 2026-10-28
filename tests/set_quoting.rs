#![cfg(unix)]

//! The shell code `lade set` prints is sourced by the pre-exec hook: a
//! value comes back byte for byte in every shell, and neither a value nor
//! a binding name can run a command.

mod common;

use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

const VALUES: [&str; 7] = [
    r"a\b",
    r"a\\b",
    r"trailing\",
    "it's",
    r"\' ; touch PWN ; \",
    "$HOME `touch PWN` $(touch PWN)",
    "line\nbreak",
];

fn set_output(home: &Path, project: &Path, shell: &str) -> std::process::Output {
    common::lade_in(home, shell)
        .current_dir(project)
        .args(["set", "mycmd"])
        .output()
        .unwrap()
}

#[test]
fn values_round_trip_in_every_shell() {
    for shell in ["zsh", "bash", "fish"] {
        for value in VALUES {
            let home = tempdir().unwrap();
            let project = tempdir().unwrap();
            let yaml = serde_json::to_string(&format!("!{value}")).unwrap();
            common::write_yml_raw(project.path(), &format!("\"mycmd\":\n  VAL: {yaml}\n"));
            let set = set_output(home.path(), project.path(), shell);
            assert!(set.status.success(), "{shell} {value:?}: {set:?}");
            let script = String::from_utf8(set.stdout).unwrap();
            let print = if shell == "fish" {
                "printf '%s' $VAL"
            } else {
                "printf '%s' \"$VAL\""
            };
            let sourced = Command::new(shell)
                .args(["-c", &format!("{script}\n{print}")])
                .current_dir(project.path())
                .output()
                .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&sourced.stdout),
                value,
                "{shell}: {}",
                String::from_utf8_lossy(&sourced.stderr)
            );
            assert!(
                !project.path().join("PWN").exists(),
                "{shell} ran {value:?}"
            );
        }
    }
}

#[test]
fn an_invalid_env_name_is_refused() {
    let home = tempdir().unwrap();
    let project = tempdir().unwrap();
    common::write_yml_raw(project.path(), "\"mycmd\":\n  \"X;touch PWN;Y\": val\n");
    let set = set_output(home.path(), project.path(), "bash");
    let stderr = String::from_utf8_lossy(&set.stderr);
    assert!(!set.status.success(), "{stderr}");
    assert!(stderr.contains("not a valid env var name"), "{stderr}");
    let script = String::from_utf8_lossy(&set.stdout);
    let _ = Command::new("bash")
        .args(["-c", &script])
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(!project.path().join("PWN").exists());
}
