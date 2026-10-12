use super::super::handle;
use super::{assert_wraps_with_ticket, test_config, with_cursor_env, with_ticket_tmpdir};
use crate::config::{Audience, LadeFile};
use crate::ticket::{self, PreEvent};
use serde_json::json;
use tempfile::tempdir;

fn hook(command: &str) -> String {
    format!(r#"{{"tool_input": {{"command": "{command}"}}}}"#)
}

fn rewritten(result: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(result).ok()?;
    parsed
        .get("updated_input")
        .and_then(|v| v.get("command"))
        .and_then(|c| c.as_str())
        .map(str::to_string)
}

fn allows(result: &str) -> bool {
    result.contains("allow") && !result.contains("updated_input")
}

#[test]
fn raw_tofu_wraps_with_ticket() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^tofu");
            let result = handle(&config, &hook("tofu apply"), Audience::Agent, None).unwrap();
            assert_wraps_with_ticket(&result, "tofu apply");
            assert!(!result.contains("lade --pretool inject"), "{result}");
        });
    });
}

#[test]
fn raw_unmatched_allows() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^tofu");
            let result = handle(&config, &hook("echo hi"), Audience::Agent, None).unwrap();
            assert!(allows(&result), "{result}");
        });
    });
}

#[test]
fn inject_tofu_wraps_inner_not_stamp() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^tofu");
            let inner = "tofu -chdir=akss apply";
            let result = handle(
                &config,
                &hook(&format!("lade inject -- {inner}")),
                Audience::Agent,
                None,
            )
            .unwrap();
            assert_wraps_with_ticket(&result, inner);
            assert!(!result.contains("lade --pretool inject"), "{result}");
            assert!(!result.contains("lade inject"), "{result}");
        });
    });
}

#[test]
fn stamped_pretool_inject_peels() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^tofu");
            let result = handle(
                &config,
                &hook("lade --pretool inject -- tofu apply"),
                Audience::Agent,
                None,
            )
            .unwrap();
            assert_wraps_with_ticket(&result, "tofu apply");
            assert!(!result.contains("lade --pretool inject"), "{result}");
        });
    });
}

#[test]
fn dashdash_alias_peels() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^tofu");
            let result =
                handle(&config, &hook("lade -- tofu apply"), Audience::Agent, None).unwrap();
            assert_wraps_with_ticket(&result, "tofu apply");
        });
    });
}

#[test]
fn inject_alias_lade_tofu_does_not_nest() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^tofu");
            let result = handle(&config, &hook("lade tofu apply"), Audience::Agent, None).unwrap();
            assert_wraps_with_ticket(&result, "tofu apply");
            let cmd = rewritten(&result).unwrap();
            assert!(!cmd.contains("lade tofu"), "{cmd}");
        });
    });
}

#[test]
fn unmatched_inject_rewrites_to_inner() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^tofu");
            let result = handle(
                &config,
                &hook("lade inject -- echo hi"),
                Audience::Agent,
                None,
            )
            .unwrap();
            assert_eq!(rewritten(&result).as_deref(), Some("echo hi"));
            assert!(!result.contains("--pretool"), "{result}");
        });
    });
}

#[test]
fn approve_prefix_kept_protocol_prefix_dropped() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config(".*");
            let result = handle(
                &config,
                &hook("LADE_APPROVE=ab12c LADE_VIA=pretool /usr/bin/lade inject 'echo'"),
                Audience::Agent,
                None,
            )
            .unwrap();
            let cmd = rewritten(&result).expect(&result);
            assert!(cmd.contains("LADE_APPROVE=ab12c "), "{cmd}");
            assert!(!cmd.contains("LADE_VIA="), "{cmd}");
            assert_wraps_with_ticket(&result, "echo");
        });
    });
}

#[test]
fn verbs_are_not_peeled_onto_an_inner_rule() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^tofu");
            for command in [
                "lade status",
                "lade hook",
                "lade --pretool mcp -- acme",
                "lade --help inject -- tofu apply",
            ] {
                let result = handle(&config, &hook(command), Audience::Agent, None).unwrap();
                assert!(allows(&result), "{command}: {result}");
            }
        });
    });
}

#[test]
fn verb_still_matches_the_full_line() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^lade status");
            let result = handle(&config, &hook("lade status"), Audience::Agent, None).unwrap();
            assert_wraps_with_ticket(&result, "lade status");
        });
    });
}

#[test]
fn inject_flags_do_not_hide_the_inner_rule() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^tofu");
            for command in [
                "lade inject --no-mask -- tofu apply",
                "lade inject --mask-format REDACTED -- tofu apply",
                "lade inject --no-mask tofu apply",
            ] {
                let result = handle(&config, &hook(command), Audience::Agent, None).unwrap();
                assert_wraps_with_ticket(&result, "tofu apply");
            }
        });
    });
}

#[test]
fn finished_ticket_wrap_is_unchanged() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^echo");
            let pre = PreEvent {
                command: "echo hello".into(),
                cwd: std::env::current_dir().unwrap(),
                via: "pretool".into(),
                audience: "agent".into(),
                actor: None,
                log: false,
                disclaimers: vec![],
                secrets: vec![],
                network: vec![],
                matches: json!([]),
                op_sa: None,
                agent: json!({}),
                network_pids: vec![],
                pending: false,
            };
            let id = ticket::write(&pre).unwrap();
            let command = format!("lade --pretool={id} 'echo hello'");
            let result = handle(&config, &hook(&command), Audience::Agent, None).unwrap();
            assert!(allows(&result), "{result}");
        });
    });
}

#[test]
fn fake_ticket_id_is_rewrapped() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^echo");
            let result = handle(
                &config,
                &hook("lade --pretool=x7Km 'echo hello'"),
                Audience::Agent,
                None,
            )
            .unwrap();
            assert_wraps_with_ticket(&result, "echo hello");
            assert!(!result.contains("--pretool=x7Km"), "{result}");
        });
    });
}

#[test]
fn ticket_write_failure_falls_back_and_keeps_pretool() {
    with_cursor_env(|| {
        let dir = tempdir().unwrap();
        let blocker = dir.path().join("not-a-dir");
        std::fs::write(&blocker, "x").unwrap();
        temp_env::with_var("LADE_TICKET_DIR", Some(blocker.to_str().unwrap()), || {
            let (config, _cfg) = test_config("^echo");
            let result = handle(&config, &hook("echo hello"), Audience::Agent, None).unwrap();
            let cmd = rewritten(&result).expect(&result);
            assert!(cmd.contains("--pretool -- 'echo hello'"), "{cmd}");
        });
    });
}

#[test]
fn empty_inject_allows() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config(".*");
            let result = handle(&config, &hook("lade inject"), Audience::Agent, None).unwrap();
            assert!(allows(&result), "{result}");
        });
    });
}

#[test]
fn log_only_inject_rewrites_to_inner() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let dir = tempdir().unwrap();
            std::fs::write(dir.path().join("lade.yml"), ".:\n  .:\n    log: true\n").unwrap();
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let result = handle(
                &config,
                &hook("lade inject -- gcloud auth list"),
                Audience::Agent,
                None,
            )
            .unwrap();
            assert_eq!(rewritten(&result).as_deref(), Some("gcloud auth list"));
            assert!(!result.contains("--pretool"), "{result}");
        });
    });
}
