use super::*;

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
