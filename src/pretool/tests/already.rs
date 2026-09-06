use super::super::handle;
use super::super::platform::is_already_injected;
use super::{test_config, with_cursor_env};
use crate::config::Audience;

#[test]
fn test_already_wrapped_stamps_via() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "lade inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("updated_input"));
        assert!(result.contains("lade --pretool inject 'echo'"));
        assert!(!result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_already_wrapped_absolute_path_stamps_via() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/local/bin/lade inject 'echo hello'"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("updated_input"));
        assert!(result.contains("/usr/local/bin/lade --pretool inject 'echo hello'"));
        assert!(!result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_already_injected_detects_lade_binaries() {
    assert!(is_already_injected("lade --pretool echo"));
    assert!(is_already_injected("lade --pretool=x7Km 'echo hello'"));
    assert!(is_already_injected("lade inject 'echo'"));
    assert!(is_already_injected("/usr/local/bin/lade inject -- echo"));
    assert!(!is_already_injected("lade hook"));
    assert!(!is_already_injected("lade status"));
    assert!(!is_already_injected("echo lade inject"));
    assert!(is_already_injected("/usr/bin/lade.exe inject echo"));
    assert!(is_already_injected(
        "LADE_APPROVE=ab12c /usr/bin/lade inject echo"
    ));
}

#[test]
fn test_env_prefix_already_injected_stamps_via() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input =
            r#"{"tool_input": {"command": "LADE_APPROVE=ab12c /usr/bin/lade inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("updated_input"));
        assert!(result.contains("LADE_APPROVE=ab12c "));
        assert!(result.contains("/usr/bin/lade --pretool inject 'echo'"));
        assert!(!result.contains("LADE_VIA=pretool "));
    });
}

#[test]
fn test_hook_stamp_already_injected_skips() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input =
            r#"{"tool_input": {"command": "LADE_VIA=pretool /usr/bin/lade inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_pretool_flag_already_injected_skips() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/bin/lade --pretool 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_pretool_equals_id_already_injected_skips() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/bin/lade --pretool=x7Km 'echo hello'"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
        assert!(!result.contains("--pretool=x7Km --pretool"));
    });
}

#[test]
fn test_via_flag_already_injected_skips() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/bin/lade --via=pretool inject 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}

#[test]
fn test_via_flag_after_inject_already_injected_skips() {
    with_cursor_env(|| {
        let (config, _dir) = test_config(".*");
        let input = r#"{"tool_input": {"command": "/usr/bin/lade inject --via=pretool 'echo'"}}"#;
        let result = handle(&config, input, Audience::Agent, None).unwrap();
        assert!(result.contains("allow"));
        assert!(!result.contains("updated_input"));
    });
}
