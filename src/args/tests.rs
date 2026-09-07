use super::*;
use clap::CommandFactory;
use std::ffi::OsString;
use std::time::Duration;

#[test]
fn bench_timeout_defaults_to_five_seconds() {
    let args = Args::try_parse_from(["lade", "bench"]).unwrap();
    match args.command {
        Some(Command::Bench(bench)) => {
            assert!(!bench.json);
            assert_eq!(bench.timeout, Duration::from_secs(5));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn bench_timeout_parses_ms() {
    let args = Args::try_parse_from(["lade", "bench", "--timeout", "500ms"]).unwrap();
    match args.command {
        Some(Command::Bench(bench)) => assert_eq!(bench.timeout, Duration::from_millis(500)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn parse_timeout_rejects_zero_and_bare_number() {
    assert!(parse_timeout("0s").is_err());
    assert!(parse_timeout("5").is_err());
}

#[test]
fn pretool_with_ticket_id_space_form() {
    std::fs::create_dir_all(crate::ticket::dir()).unwrap();
    std::fs::write(crate::ticket::path("x7Km"), "{}").unwrap();
    let (ticket_id, argv) = crate::ticket::peel_pretool(vec![
        OsString::from("lade"),
        OsString::from("--pretool"),
        OsString::from("x7Km"),
        OsString::from("inject"),
        OsString::from("echo"),
    ]);
    let _ = crate::ticket::unlink("x7Km");
    assert_eq!(ticket_id.as_deref(), Some("x7Km"));
    let args = Args::try_parse_from(&argv).unwrap();
    assert!(args.pretool);
    match args.command {
        Some(Command::Inject(inject)) => assert_eq!(inject.commands, vec!["echo"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pretool_with_ticket_id_equals_form() {
    let (ticket_id, argv) = crate::ticket::peel_pretool(vec![
        OsString::from("lade"),
        OsString::from("--pretool=x7Km"),
        OsString::from("inject"),
        OsString::from("echo"),
    ]);
    assert_eq!(ticket_id.as_deref(), Some("x7Km"));
    let args = Args::try_parse_from(&argv).unwrap();
    assert!(args.pretool);
    match args.command {
        Some(Command::Inject(inject)) => assert_eq!(inject.commands, vec!["echo"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pretool_before_inject() {
    let args = Args::try_parse_from(["lade", "--pretool", "inject", "echo"]).unwrap();
    assert!(args.pretool);
    match args.command {
        Some(Command::Inject(inject)) => assert_eq!(inject.commands, vec!["echo"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pretool_before_alias() {
    let args = Args::try_parse_from(["lade", "--pretool", "terraform", "apply"]).unwrap();
    assert!(args.pretool);
    match args.command {
        Some(Command::InjectAlias(commands)) => {
            assert_eq!(commands, vec!["terraform", "apply"])
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn pretool_before_mcp() {
    let args = Args::try_parse_from(["lade", "--pretool", "mcp", "--", "acme"]).unwrap();
    assert!(args.pretool);
    match args.command {
        Some(Command::Mcp(mcp)) => assert_eq!(mcp.argv, vec![OsString::from("acme")]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn log_is_not_inject_alias() {
    let args = Args::try_parse_from(["lade", "log", "--since", "1d"]).unwrap();
    match args.command {
        Some(Command::Log(log)) => assert_eq!(log.since.as_deref(), Some("1d")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn help_with_verbose_flag_lists_internal() {
    let quiet = Args::try_parse_from(["lade", "--help"]).unwrap();
    assert!(quiet.help);
    assert!(!help_lists_internal(&quiet.verbose));
    let short = Args::try_parse_from(["lade", "--help", "-v"]).unwrap();
    assert!(short.help);
    assert!(help_lists_internal(&short.verbose));
    let long = Args::try_parse_from(["lade", "--help", "--verbose"]).unwrap();
    assert!(long.help);
    assert!(help_lists_internal(&long.verbose));
}

#[test]
fn default_help_hides_internal_commands() {
    let help = Args::command()
        .after_help(super::INTERNAL_HINT)
        .render_help()
        .to_string();
    assert!(help.contains("Internal commands: lade --help -v"));
    assert!(!help.contains("\n  set "));
    assert!(!help.contains("\n  unset "));
    assert!(help.contains("\n  hook "));
    assert!(!help.contains("--pretool"));
}

#[test]
fn verbose_help_lists_internal_commands() {
    let mut cmd = Args::command();
    super::reveal_internal(&mut cmd);
    let help = cmd.render_help().to_string();
    assert!(help.contains("  set "));
    assert!(help.contains("  unset "));
    assert!(help.contains("  hook "));
    assert!(help.contains("--pretool"));
    assert!(!help.contains("Internal commands: lade --help -v"));
}

#[test]
fn hook_stdin_still_parses_harness() {
    let args = Args::try_parse_from(["lade", "hook", "--harness", "cursor"]).unwrap();
    match args.command {
        Some(Command::Hook {
            harness: Some(name),
            action: None,
        }) => assert_eq!(name, "cursor"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn hook_install_defaults_scope_to_project() {
    assert!(Args::try_parse_from(["lade", "hook", "install"]).is_err());
    let args = Args::try_parse_from(["lade", "hook", "install", "--harness", "cursor"]).unwrap();
    match args.command {
        Some(Command::Hook {
            action: Some(HookAction::Install(opts)),
            harness: None,
        }) => {
            assert_eq!(opts.scope, HookScope::Project);
            assert_eq!(opts.harness, HookHarness::Cursor);
        }
        other => panic!("{other:?}"),
    }
    let user = Args::try_parse_from([
        "lade",
        "hook",
        "install",
        "--scope",
        "user",
        "--harness",
        "cursor",
    ])
    .unwrap();
    match user.command {
        Some(Command::Hook {
            action: Some(HookAction::Install(opts)),
            ..
        }) => assert_eq!(opts.scope, HookScope::User),
        other => panic!("{other:?}"),
    }
}

#[test]
fn install_help_names_preexec_and_pretool() {
    let mut cmd = Args::command();
    let install = cmd.find_subcommand_mut("install").expect("install");
    let mut buf = Vec::new();
    install.write_long_help(&mut buf).unwrap();
    let help = String::from_utf8(buf).unwrap();
    assert!(help.contains("pre-exec"), "{help}");
    assert!(help.contains("pre-tool"), "{help}");
}

#[test]
fn install_agent_flags_select_slugs() {
    let bare = Args::try_parse_from(["lade", "install"]).unwrap();
    match bare.command {
        Some(Command::Install(opts)) => assert!(opts.slugs().is_empty()),
        other => panic!("{other:?}"),
    }
    let args = Args::try_parse_from(["lade", "install", "--cursor", "--opencode"]).unwrap();
    match args.command {
        Some(Command::Install(opts)) => {
            assert_eq!(opts.slugs(), vec!["cursor", "opencode"]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn usage_all_and_path_conflict() {
    assert!(Args::try_parse_from(["lade", "usage", "--all", "--path", "/tmp"]).is_err());
    let args = Args::try_parse_from(["lade", "usage", "--all"]).unwrap();
    match args.command {
        Some(Command::Usage(usage)) => {
            assert!(usage.all);
            assert!(usage.path.is_none());
        }
        other => panic!("{other:?}"),
    }
}
