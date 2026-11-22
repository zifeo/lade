use super::*;
use crate::args::{
    DEFAULT_MASK_FORMAT, EvalCommand, HookAction, HookAgent, HookToggleCommand, InjectCommand,
};
use crate::preexec::LADE_VIA;

const SIGNALS: [&str; 14] = [
    "AI_AGENT",
    "AGENT",
    "CLAUDECODE",
    "CLAUDE_CODE",
    "CURSOR_AGENT",
    "CURSOR_EXTENSION_HOST_ROLE",
    "CURSOR_SANDBOX",
    "COPILOT_MODEL",
    "CURSOR_VERSION",
    "CODEX_THREAD_ID",
    "CODEX_SANDBOX",
    "CODEX_CI",
    "OPENCODE",
    "OPENCODE_PID",
];

fn cleared_signals() -> Vec<(&'static str, Option<&'static str>)> {
    let mut vars: Vec<_> = SIGNALS.iter().map(|k| (*k, None)).collect();
    vars.push((LADE_VIA, None));
    vars
}

fn inject() -> Command {
    Command::Inject(InjectCommand {
        no_mask: false,
        mask_format: DEFAULT_MASK_FORMAT.into(),
        commands: vec!["x".into()],
    })
}

fn set_cmd() -> Command {
    Command::Set(EvalCommand {
        commands: vec!["x".into()],
    })
}

#[test]
fn mcp_command_is_mcp_agent() {
    temp_env::with_vars(cleared_signals(), || {
        let d = detect(
            &Command::Mcp(crate::args::McpCommand {
                url: None,
                argv: vec!["env".into()],
            }),
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(d.via, Via::Mcp);
        assert_eq!(d.audience, Audience::Agent);
        assert_eq!(d.ui, UiMode::Quiet);
    });
}

#[test]
fn leftover_via_env_does_not_classify() {
    temp_env::with_vars(
        cleared_signals()
            .into_iter()
            .map(|(k, v)| {
                if k == LADE_VIA {
                    (k, Some(Via::PRETOOL))
                } else {
                    (k, v)
                }
            })
            .collect::<Vec<_>>(),
        || {
            let d = detect(&inject(), false, true, true).unwrap();
            assert_eq!(d.via, Via::Organic);
            assert_eq!(d.audience, Audience::Human);
        },
    );
}

#[test]
fn inject_via_flag_is_pretool_agent_quiet() {
    temp_env::with_vars(cleared_signals(), || {
        let d = detect(&inject(), true, true, true).unwrap();
        assert_eq!(d.via, Via::Pretool);
        assert_eq!(d.audience, Audience::Agent);
        assert_eq!(d.ui, UiMode::Quiet);
    });
}

#[test]
fn via_flag_on_status_is_pretool_agent() {
    temp_env::with_vars(cleared_signals(), || {
        let d = detect(
            &Command::Status(crate::args::StatusCommand {
                all: false,
                json: false,
            }),
            true,
            true,
            true,
        )
        .unwrap();
        assert_eq!(d.via, Via::Pretool);
        assert_eq!(d.audience, Audience::Agent);
        assert_eq!(d.ui, UiMode::Quiet);
    });
}

#[test]
fn via_flag_wins_over_env() {
    temp_env::with_vars(
        cleared_signals()
            .into_iter()
            .map(|(k, v)| {
                if k == LADE_VIA {
                    (k, Some(Via::PREEXEC))
                } else {
                    (k, v)
                }
            })
            .collect::<Vec<_>>(),
        || {
            let d = detect(&inject(), true, true, true).unwrap();
            assert_eq!(d.via, Via::Pretool);
            assert_eq!(d.audience, Audience::Agent);
        },
    );
}

#[test]
fn set_is_preexec_human_quiet() {
    temp_env::with_vars(cleared_signals(), || {
        let d = detect(&set_cmd(), false, true, true).unwrap();
        assert_eq!(d.via, Via::Preexec);
        assert_eq!(d.audience, Audience::Human);
        assert_eq!(d.ui, UiMode::Quiet);
    });
}

#[test]
fn hook_is_pretool_agent_quiet() {
    temp_env::with_vars(cleared_signals(), || {
        let d = detect(
            &Command::Hook {
                harness: None,
                action: None,
            },
            false,
            false,
            false,
        )
        .unwrap();
        assert_eq!(d.via, Via::Pretool);
        assert_eq!(d.audience, Audience::Agent);
        assert_eq!(d.ui, UiMode::Quiet);
    });
}

#[test]
fn hook_enable_is_not_pretool() {
    temp_env::with_vars(cleared_signals(), || {
        let d = detect(
            &Command::Hook {
                harness: None,
                action: Some(HookAction::Enable(HookToggleCommand {
                    shell: false,
                    agent: Some(HookAgent::Cursor),
                    global: true,
                })),
            },
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(d.via, Via::Organic);
        assert_eq!(d.audience, Audience::Human);
        assert_eq!(d.ui, UiMode::Quiet);
    });
}

#[test]
fn empty_via_with_signal_is_agent_quiet() {
    temp_env::with_vars(
        [
            (LADE_VIA, None),
            ("AI_AGENT", None),
            ("AGENT", None),
            ("CLAUDECODE", None),
            ("CURSOR_AGENT", Some("1")),
            ("COPILOT_MODEL", None),
            ("CURSOR_VERSION", None),
        ],
        || {
            let d = detect(&inject(), false, true, true).unwrap();
            assert_eq!(d.via, Via::Unknown);
            assert_eq!(d.audience, Audience::Agent);
            assert_eq!(d.ui, UiMode::Quiet);
        },
    );
}

#[test]
fn empty_via_without_signal_inject_tty_is_organic_human_interactive() {
    temp_env::with_vars(cleared_signals(), || {
        let d = detect(&inject(), false, true, true).unwrap();
        assert_eq!(d.via, Via::Organic);
        assert_eq!(d.audience, Audience::Human);
        assert_eq!(d.ui, UiMode::Interactive);
    });
}

#[test]
fn empty_via_without_signal_or_tty_is_unknown_human_quiet() {
    temp_env::with_vars(cleared_signals(), || {
        let d = detect(&inject(), false, false, false).unwrap();
        assert_eq!(d.via, Via::Unknown);
        assert_eq!(d.audience, Audience::Human);
        assert_eq!(d.ui, UiMode::Quiet);
    });
}

#[test]
fn cursor_version_alone_is_human() {
    let mut vars = cleared_signals();
    for (key, value) in vars.iter_mut() {
        if *key == "CURSOR_VERSION" {
            *value = Some("1.0");
        }
    }
    temp_env::with_vars(vars, || {
        let d = detect(&inject(), false, true, true).unwrap();
        assert_eq!(d.via, Via::Organic);
        assert_eq!(d.audience, Audience::Human);
        assert_eq!(d.ui, UiMode::Interactive);
    });
}

#[test]
fn leftover_via_garbage_is_ignored() {
    temp_env::with_var(LADE_VIA, Some("nope"), || {
        let d = detect(&inject(), false, false, false).unwrap();
        assert_eq!(d.via, Via::Unknown);
    });
}

#[test]
fn ai_agent_takes_precedence() {
    let mut vars = cleared_signals();
    for (key, value) in vars.iter_mut() {
        match *key {
            "AI_AGENT" => *value = Some("claude-code"),
            "AGENT" => *value = Some("goose"),
            "CLAUDECODE" => *value = Some("1"),
            _ => {}
        }
    }
    temp_env::with_vars(vars, || {
        assert_eq!(agent_signal().as_deref(), Some("claude-code"))
    });
}

/// `(variable, value, harness)` for each signal of the `docs/env.md` table.
fn documented_signals() -> Vec<(String, String, String)> {
    let env_md = include_str!("../../docs/env.md");
    let section = env_md
        .split("## Harness detection")
        .nth(1)
        .expect("docs/env.md has a Harness detection section");
    let mut signals = Vec::new();
    for line in section.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (variables, harness) = (cells[1], cells[2]);
        for variable in variables.split(", ") {
            let variable = variable.trim_matches('`');
            let (key, value) = variable.split_once('=').unwrap_or((variable, "1"));
            let value = if harness == "value of the variable" {
                "some-harness"
            } else {
                value
            };
            let harness = if harness == "value of the variable" {
                value
            } else {
                harness
            };
            signals.push((key.to_string(), value.to_string(), harness.to_string()));
        }
    }
    assert!(signals.len() >= 13, "{signals:?}");
    signals
}

#[test]
fn each_documented_signal_is_a_harness() {
    for (key, value, harness) in documented_signals() {
        assert!(
            SIGNALS.contains(&key.as_str()),
            "{key} is not cleared by the tests"
        );
        let vars: Vec<(&str, Option<&str>)> = cleared_signals()
            .into_iter()
            .map(|(name, set)| match name == key {
                true => (name, Some(value.as_str())),
                false => (name, set),
            })
            .collect();
        temp_env::with_vars(vars, || {
            assert_eq!(agent_signal().as_deref(), Some(harness.as_str()), "{key}");
            let d = detect(&inject(), false, false, false).unwrap();
            assert_eq!(d.audience, Audience::Agent, "{key}");
        });
    }
}

#[test]
fn undocumented_harness_variables_are_human() {
    for (key, value) in [
        ("CURSOR_EXTENSION_HOST_ROLE", "other"),
        ("CODEX_HOME", "/home/me/.codex"),
        ("OPENCODE_DIR", "/home/me/.opencode"),
    ] {
        let mut vars = cleared_signals();
        vars.retain(|(name, _)| *name != key);
        vars.push((key, Some(value)));
        temp_env::with_vars(vars, || {
            assert_eq!(agent_signal(), None, "{key}");
            let d = detect(&inject(), false, false, false).unwrap();
            assert_eq!(d.audience, Audience::Human, "{key}");
        });
    }
}

#[test]
fn claudecode_non_one_is_ignored() {
    let mut vars = cleared_signals();
    for (key, value) in vars.iter_mut() {
        if *key == "CLAUDECODE" {
            *value = Some("0");
        }
    }
    temp_env::with_vars(vars, || assert_eq!(agent_signal(), None));
}

#[test]
fn child_stamp_matches_via() {
    assert_eq!(Via::Pretool.child_stamp(), Some(Via::PRETOOL));
    assert_eq!(Via::Preexec.child_stamp(), Some(Via::PREEXEC));
    assert_eq!(Via::Organic.child_stamp(), None);
    assert_eq!(Via::Unknown.child_stamp(), None);
    assert_eq!(Via::Mcp.child_stamp(), None);
}
