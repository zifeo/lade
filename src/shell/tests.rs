use super::*;
use std::collections::HashMap;

#[test]
fn test_set_bash_single_key() {
    let result = Shell::Bash.set(HashMap::from([("KEY".to_string(), "value".to_string())]));
    assert_eq!(result, "export KEY='value'");
}

#[test]
fn test_set_zsh_single_key() {
    assert_eq!(
        Shell::Zsh.set(HashMap::from([("KEY".to_string(), "value".to_string())])),
        "export KEY='value'"
    );
}

#[test]
fn test_set_fish_single_key() {
    assert_eq!(
        Shell::Fish.set(HashMap::from([("KEY".to_string(), "value".to_string())])),
        "set --global --export KEY 'value'"
    );
}

#[test]
fn test_set_empty_map() {
    assert_eq!(Shell::Bash.set(HashMap::new()), "");
}

#[test]
fn test_set_multiple_keys_contains() {
    let result = Shell::Bash.set(HashMap::from([
        ("A".to_string(), "1".to_string()),
        ("B".to_string(), "2".to_string()),
    ]));
    assert!(result.contains("export A='1'") && result.contains("export B='2'"));
    assert!(result.contains(';'));
}

#[test]
fn test_unset_bash_single_key() {
    assert_eq!(Shell::Bash.unset(vec!["KEY".to_string()]), "unset -v KEY");
}

#[test]
fn test_unset_fish_single_key() {
    assert_eq!(
        Shell::Fish.unset(vec!["KEY".to_string()]),
        "set --global --erase KEY"
    );
}

#[test]
fn test_unset_multiple_keys_order_preserved() {
    assert_eq!(
        Shell::Bash.unset(vec!["KEY1".to_string(), "KEY2".to_string()]),
        "unset -v KEY1;unset -v KEY2"
    );
}

#[test]
fn test_restore_replaces_and_erases() {
    let previous = HashMap::from([
        ("KEEP".to_string(), Some("sock".to_string())),
        ("DROP".to_string(), None),
    ]);
    let result = Shell::Bash.restore(previous);
    assert!(result.contains("export KEEP='sock'"));
    assert!(result.contains("unset -v DROP"));
}

#[test]
fn test_restore_payload_roundtrip() {
    let payload = RestorePayload {
        env: HashMap::from([
            (
                "SSH_AUTH_SOCK".to_string(),
                Some("/tmp/agent.sock".to_string()),
            ),
            ("NEW".to_string(), None),
        ]),
    };
    let decoded = RestorePayload::decode(&payload.encode().unwrap()).unwrap();
    assert_eq!(payload, decoded);
    assert!(RestorePayload::decode("not-v1").is_err());
    assert!(RestorePayload::decode("v1:!!!").is_err());
}

#[test]
fn test_restore_fish_syntax() {
    let previous = HashMap::from([
        ("KEEP".to_string(), Some("sock".to_string())),
        ("DROP".to_string(), None),
    ]);
    let result = Shell::Fish.restore(previous);
    assert!(result.contains("set --global --export KEEP 'sock'"));
    assert!(result.contains("set --global --erase DROP"));
}

#[test]
fn test_set_escaping() {
    let env = HashMap::from([("KEY".to_string(), "val'ue".to_string())]);
    let result = Shell::Bash.set(env);
    assert_eq!(result, "export KEY='val'\\''ue'");
}

#[test]
fn wrap_startup_file_is_shell_specific() {
    assert!(
        Shell::Fish
            .wrap_startup_file()
            .unwrap()
            .ends_with(".config/fish/config.fish")
    );
    assert!(Shell::Zsh.wrap_startup_file().unwrap().ends_with(".zshenv"));
    assert!(Shell::Bash.wrap_startup_file().is_none());
    assert!(Shell::Sh.wrap_startup_file().is_none());
}

#[test]
fn noninteractive_args_skip_startup_files() {
    assert_eq!(
        Shell::Fish.noninteractive_args("echo hi"),
        vec!["--no-config", "-c", "echo hi"]
    );
    assert_eq!(
        Shell::Bash.noninteractive_args("echo hi"),
        vec!["-c", "echo hi"]
    );
    assert_eq!(
        Shell::Zsh.noninteractive_args("echo hi"),
        vec!["-f", "-c", "echo hi"]
    );
    assert_eq!(
        Shell::Sh.noninteractive_args("echo hi"),
        vec!["-c", "echo hi"]
    );
}

#[test]
fn comm_maps_to_shell() {
    assert!(matches!(shell_from_comm("zsh").unwrap(), Shell::Zsh));
    assert!(matches!(shell_from_comm("bash.exe").unwrap(), Shell::Bash));
    assert!(matches!(shell_from_comm("/bin/fish").unwrap(), Shell::Fish));
    assert!(shell_from_comm("login").is_err());
}

#[test]
fn on_exports_lade_shell() {
    let zsh = Shell::Zsh.on().unwrap();
    assert!(zsh.starts_with("export LADE_SHELL=zsh\n"));
    let bash = Shell::Bash.on().unwrap();
    assert!(bash.starts_with("export LADE_SHELL=bash\n"));
    let fish = Shell::Fish.on().unwrap();
    assert!(fish.starts_with("set --global --export LADE_SHELL fish\n"));
}

#[test]
fn off_unsets_lade_shell() {
    assert!(
        Shell::Zsh
            .off()
            .unwrap()
            .starts_with("unset -v LADE_SHELL\n")
    );
    assert!(
        Shell::Fish
            .off()
            .unwrap()
            .starts_with("set --global --erase LADE_SHELL\n")
    );
}
