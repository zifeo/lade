use super::server::should_exit_for_test;
use super::tables::{
    age_ledger_for_test, empty_tables, handle_for_test, take_version_fetch_for_test,
};
use super::wire::{self, InReq, Rep, Req};

#[test]
fn wire_roundtrip_get() {
    let req = Req::Get {
        cwd: "/tmp".into(),
        path: "/tmp/lade.yaml".into(),
        rule: "terraform .*".into(),
        when: "always".into(),
        walk: [3u8; 32],
        user: "alice".into(),
    };
    let frame = wire::encode_req(&req).unwrap();
    assert_eq!(&frame[0..4], b"LDH1");
    assert!(matches!(wire::decode_in_req(&frame), Some(InReq::Body(got)) if *got == req));
}

#[test]
fn wire_rejects_bad_magic() {
    let mut frame = wire::encode_req(&Req::Ping).unwrap();
    frame[0] = b'X';
    assert!(wire::decode_in_req(&frame).is_none());
}

#[test]
fn wire_other_bin_ver_is_denied() {
    let frame = wire::encode_req_ver(&Req::Ping, "0.0.0").unwrap();
    assert!(matches!(wire::decode_in_req(&frame), Some(InReq::WrongVer)));
    let reply = wire::encode_rep_ver(&Rep::Ok, "0.0.0").unwrap();
    assert_eq!(wire::decode_rep(&reply), Some(Rep::Err(wire::ERR_DENIED)));
}

#[test]
fn tables_put_get_and_ttl() {
    let mut tables = empty_tables();
    let walk = [1u8; 32];
    let put = Req::Put {
        cwd: "/c".into(),
        path: "/c/lade.yaml".into(),
        rule: "t .*".into(),
        when: "always".into(),
        walk,
        user: String::new(),
        ttl_ms: 60_000,
        bindings: vec![("AWS".into(), b"cipher".to_vec())],
    };
    assert_eq!(handle_for_test(&mut tables, put), Rep::Ok);
    let get = Req::Get {
        cwd: "/c".into(),
        path: "/c/lade.yaml".into(),
        rule: "t .*".into(),
        when: "always".into(),
        walk,
        user: String::new(),
    };
    match handle_for_test(&mut tables, get) {
        Rep::Hit { bindings } => assert_eq!(bindings[0].0, "AWS"),
        other => panic!("{other:?}"),
    }
}

fn cap_put(ttl_ms: u32) -> Req {
    Req::Put {
        cwd: "/c".into(),
        path: "/c/lade.yaml".into(),
        rule: "t .*".into(),
        when: "always".into(),
        walk: [9u8; 32],
        user: String::new(),
        ttl_ms,
        bindings: vec![("AWS".into(), b"cipher".to_vec())],
    }
}

fn listed_ttl_left(tables: &mut super::tables::Tables) -> Vec<u32> {
    super::tables::check_cap_for_test(tables);
    match handle_for_test(tables, Req::List) {
        Rep::Listing { rows, .. } => rows.iter().map(|row| row.ttl_left_ms).collect(),
        other => panic!("{other:?}"),
    }
}

/// A new inode and size: the hub's stamp moves even within one mtime tick.
fn write_config(path: &std::path::Path, text: &str) {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

#[test]
fn max_cache_caps_secret_ttls() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    write_config(&config, r#"{"max_cache":"1m"}"#);
    let mut tables = super::tables::tables_with_config(config);
    assert_eq!(handle_for_test(&mut tables, cap_put(3_600_000)), Rep::Ok);
    let left = listed_ttl_left(&mut tables);
    assert_eq!(left.len(), 1);
    assert!(left[0] <= 60_000, "{left:?}");
}

#[test]
fn max_cache_zero_keeps_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    write_config(&config, r#"{"max_cache":"0"}"#);
    let mut tables = super::tables::tables_with_config(config);
    assert_eq!(handle_for_test(&mut tables, cap_put(60_000)), Rep::Ok);
    assert!(listed_ttl_left(&mut tables).is_empty());
}

#[test]
fn an_invalid_max_cache_keeps_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    write_config(&config, r#"{"max_cache":"soon"}"#);
    let mut tables = super::tables::tables_with_config(config);
    assert_eq!(handle_for_test(&mut tables, cap_put(60_000)), Rep::Ok);
    assert!(listed_ttl_left(&mut tables).is_empty());
}

#[test]
fn any_max_cache_change_drops_cached_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    write_config(&config, r#"{"max_cache":"1m"}"#);
    let mut tables = super::tables::tables_with_config(config.clone());
    handle_for_test(&mut tables, cap_put(60_000));
    // Another key leaves the cap alone: the row stays.
    write_config(&config, r#"{"user":"alice","max_cache":"1m"}"#);
    assert_eq!(listed_ttl_left(&mut tables).len(), 1);
    // Raising the cap does not extend the row: it drops it.
    write_config(&config, r#"{"user":"alice","max_cache":"2h"}"#);
    assert!(listed_ttl_left(&mut tables).is_empty());
    handle_for_test(&mut tables, cap_put(60_000));
    // So does removing the field.
    write_config(&config, r#"{"user":"alice"}"#);
    assert!(listed_ttl_left(&mut tables).is_empty());
}

#[test]
fn tables_list_and_forget_named_key() {
    let mut tables = empty_tables();
    let walk = [2u8; 32];
    let put = Req::Put {
        cwd: "/c".into(),
        path: "/c/lade.yaml".into(),
        rule: "t .*".into(),
        when: "always".into(),
        walk,
        user: String::new(),
        ttl_ms: 60_000,
        bindings: vec![("AWS".into(), b"a".to_vec()), ("DB".into(), b"b".to_vec())],
    };
    assert_eq!(handle_for_test(&mut tables, put), Rep::Ok);
    match handle_for_test(&mut tables, Req::List) {
        Rep::Listing { rows, tickets, .. } => {
            assert_eq!(tickets, 0);
            let names: Vec<_> = rows.iter().map(|row| row.name.as_str()).collect();
            assert_eq!(names, ["AWS", "DB"]);
            assert!(rows.iter().all(|row| row.cwd == "/c"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        handle_for_test(
            &mut tables,
            Req::Forget {
                names: vec!["AWS".into()],
            },
        ),
        Rep::Ok
    );
    match handle_for_test(&mut tables, Req::List) {
        Rep::Listing { rows, .. } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].name, "DB");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn tables_window_marks_scope_and_restamps() {
    let mut tables = empty_tables();
    let put = Req::Put {
        cwd: "/proj/src".into(),
        path: "/proj/lade.yaml".into(),
        rule: "t .*".into(),
        when: "always".into(),
        walk: [9u8; 32],
        user: String::new(),
        ttl_ms: 60_000,
        bindings: vec![("AWS".into(), b"a".to_vec())],
    };
    assert_eq!(handle_for_test(&mut tables, put), Rep::Ok);
    assert_eq!(
        handle_for_test(
            &mut tables,
            Req::SetWindow {
                scope: "/proj".into(),
                ttl: Some("2h".into()),
            },
        ),
        Rep::Ok
    );
    match handle_for_test(
        &mut tables,
        Req::GetWindow {
            scope: "/proj".into(),
        },
    ) {
        Rep::Window { ttl } => assert_eq!(ttl.as_deref(), Some("2h")),
        other => panic!("{other:?}"),
    }
    match handle_for_test(&mut tables, Req::List) {
        Rep::Listing { rows, .. } => {
            assert!(rows[0].ttl_left_ms > 60_000, "{}", rows[0].ttl_left_ms);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        handle_for_test(
            &mut tables,
            Req::SetWindow {
                scope: "/proj".into(),
                ttl: None,
            },
        ),
        Rep::Ok
    );
    match handle_for_test(
        &mut tables,
        Req::GetWindow {
            scope: "/proj".into(),
        },
    ) {
        Rep::Window { ttl } => assert_eq!(ttl, None),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_window_in_the_root_covers_every_directory_and_the_root_shares_a_childs() {
    let window = |tables: &mut super::tables::Tables, scope: &str| match handle_for_test(
        tables,
        Req::GetWindow {
            scope: scope.into(),
        },
    ) {
        Rep::Window { ttl } => ttl,
        other => panic!("{other:?}"),
    };
    let set = |tables: &mut super::tables::Tables, scope: &str, ttl: &str| {
        let req = Req::SetWindow {
            scope: scope.into(),
            ttl: Some(ttl.into()),
        };
        assert_eq!(handle_for_test(tables, req), Rep::Ok);
    };
    let mut tables = empty_tables();
    set(&mut tables, "/", "2h");
    assert_eq!(window(&mut tables, "/").as_deref(), Some("2h"));
    assert_eq!(window(&mut tables, "/proj/src").as_deref(), Some("2h"));

    let mut tables = empty_tables();
    set(&mut tables, "/proj", "30m");
    assert_eq!(window(&mut tables, "/").as_deref(), Some("30m"));
    set(&mut tables, "/", "2h");
    assert_eq!(window(&mut tables, "/proj/src").as_deref(), Some("30m"));
    assert_eq!(window(&mut tables, "/").as_deref(), Some("2h"));
    assert!(super::path_related("/proj", "/"));
    assert!(!super::path_related("/projx", "/proj"));
}

#[test]
fn tables_window_follows_cwd_hierarchy() {
    let mut tables = empty_tables();
    assert_eq!(
        handle_for_test(
            &mut tables,
            Req::SetWindow {
                scope: "/proj/src".into(),
                ttl: Some("2h".into()),
            },
        ),
        Rep::Ok
    );
    match handle_for_test(
        &mut tables,
        Req::GetWindow {
            scope: "/proj".into(),
        },
    ) {
        Rep::Window { ttl } => assert_eq!(ttl.as_deref(), Some("2h")),
        other => panic!("{other:?}"),
    }
    match handle_for_test(
        &mut tables,
        Req::GetWindow {
            scope: "/proj/src/lib".into(),
        },
    ) {
        Rep::Window { ttl } => assert_eq!(ttl.as_deref(), Some("2h")),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        handle_for_test(
            &mut tables,
            Req::SetWindow {
                scope: "/proj".into(),
                ttl: Some("30m".into()),
            },
        ),
        Rep::Ok
    );
    match handle_for_test(
        &mut tables,
        Req::GetWindow {
            scope: "/proj/src".into(),
        },
    ) {
        Rep::Window { ttl } => assert_eq!(ttl.as_deref(), Some("2h")),
        other => panic!("{other:?}"),
    }
    match handle_for_test(
        &mut tables,
        Req::GetWindow {
            scope: "/other".into(),
        },
    ) {
        Rep::Window { ttl } => assert_eq!(ttl, None),
        other => panic!("{other:?}"),
    }
}

#[test]
fn tables_upsert_at_cap_keeps_row() {
    let mut tables = empty_tables();
    let walk = [1u8; 32];
    for i in 0..256u16 {
        let put = Req::Put {
            cwd: format!("/c{i}"),
            path: "/c/lade.yaml".into(),
            rule: "t .*".into(),
            when: "always".into(),
            walk,
            user: String::new(),
            ttl_ms: 60_000,
            bindings: vec![("AWS".into(), vec![i as u8])],
        };
        assert_eq!(handle_for_test(&mut tables, put), Rep::Ok);
    }
    let upsert = Req::Put {
        cwd: "/c0".into(),
        path: "/c/lade.yaml".into(),
        rule: "t .*".into(),
        when: "always".into(),
        walk,
        user: String::new(),
        ttl_ms: 60_000,
        bindings: vec![("AWS".into(), vec![99])],
    };
    assert_eq!(handle_for_test(&mut tables, upsert), Rep::Ok);
    match handle_for_test(
        &mut tables,
        Req::Get {
            cwd: "/c0".into(),
            path: "/c/lade.yaml".into(),
            rule: "t .*".into(),
            when: "always".into(),
            walk,
            user: String::new(),
        },
    ) {
        Rep::Hit { bindings } => assert_eq!(bindings[0].1, vec![99]),
        other => panic!("{other:?}"),
    }
    let extra = Req::Put {
        cwd: "/extra".into(),
        path: "/c/lade.yaml".into(),
        rule: "t .*".into(),
        when: "always".into(),
        walk,
        user: String::new(),
        ttl_ms: 60_000,
        bindings: vec![("AWS".into(), vec![1])],
    };
    assert_eq!(handle_for_test(&mut tables, extra), Rep::Ok);
    assert_eq!(
        handle_for_test(
            &mut tables,
            Req::Get {
                cwd: "/c1".into(),
                path: "/c/lade.yaml".into(),
                rule: "t .*".into(),
                when: "always".into(),
                walk,
                user: String::new(),
            },
        ),
        Rep::Miss
    );
}

#[test]
fn tables_ticket_unlink() {
    let mut tables = empty_tables();
    let t = *b"ab12";
    assert_eq!(
        handle_for_test(
            &mut tables,
            Req::PutT {
                t,
                ticket: b"{}".to_vec(),
                notice: false,
            }
        ),
        Rep::Ok
    );
    match handle_for_test(&mut tables, Req::GetT { t }) {
        Rep::Ticket { ticket } => assert_eq!(ticket, b"{}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(handle_for_test(&mut tables, Req::UnlinkT { t }), Rep::Ok);
    assert_eq!(handle_for_test(&mut tables, Req::GetT { t }), Rep::Miss);
}

#[test]
fn ping_reports_table_counts() {
    let mut tables = empty_tables();
    let walk = [1u8; 32];
    assert_eq!(
        handle_for_test(&mut tables, Req::Ping),
        Rep::Stat {
            pid: std::process::id(),
            secrets: 0,
            tickets: 0,
            unguarded: None,
        }
    );
    assert_eq!(
        handle_for_test(
            &mut tables,
            Req::Put {
                cwd: "/c".into(),
                path: "/c/lade.yaml".into(),
                rule: "t .*".into(),
                when: "always".into(),
                walk,
                user: String::new(),
                ttl_ms: 60_000,
                bindings: vec![("AWS".into(), b"cipher".to_vec())],
            }
        ),
        Rep::Ok
    );
    assert_eq!(
        handle_for_test(
            &mut tables,
            Req::PutT {
                t: *b"ab12",
                ticket: b"{}".to_vec(),
                notice: false,
            }
        ),
        Rep::Ok
    );
    assert_eq!(
        handle_for_test(&mut tables, Req::Ping),
        Rep::Stat {
            pid: std::process::id(),
            secrets: 1,
            tickets: 1,
            unguarded: None,
        }
    );
    assert_eq!(handle_for_test(&mut tables, Req::Quit), Rep::Ok);
}

fn put_secret(tables: &mut super::tables::Tables, ttl_ms: u32) {
    let put = Req::Put {
        cwd: "/c".into(),
        path: "/c/lade.yaml".into(),
        rule: "t .*".into(),
        when: "always".into(),
        walk: [1u8; 32],
        user: String::new(),
        ttl_ms,
        bindings: vec![("AWS".into(), b"cipher".to_vec())],
    };
    assert_eq!(handle_for_test(tables, put), Rep::Ok);
}

#[test]
fn idle_hub_exits_only_once_tables_are_empty() {
    let mut empty = empty_tables();
    assert!(should_exit_for_test(&mut empty, true, true));

    let mut live = empty_tables();
    put_secret(&mut live, 60_000);
    assert!(!should_exit_for_test(&mut live, true, true));

    let mut expired = empty_tables();
    put_secret(&mut expired, 1);
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(should_exit_for_test(&mut expired, true, true));

    let mut ticket = empty_tables();
    let put_t = Req::PutT {
        t: *b"ab12",
        ticket: b"{}".to_vec(),
        notice: false,
    };
    assert_eq!(handle_for_test(&mut ticket, put_t), Rep::Ok);
    assert!(!should_exit_for_test(&mut ticket, true, true));

    let mut window = empty_tables();
    let set = Req::SetWindow {
        scope: "/proj".into(),
        ttl: Some("2h".into()),
    };
    assert_eq!(handle_for_test(&mut window, set), Rep::Ok);
    assert!(!should_exit_for_test(&mut window, true, true));
}

#[test]
fn reachable_hub_waits_out_the_idle_time() {
    let mut empty = empty_tables();
    assert!(!should_exit_for_test(&mut empty, true, false));
}

fn put_t(notice: bool) -> Req {
    Req::PutT {
        t: *b"ab12",
        ticket: b"{}".to_vec(),
        notice,
    }
}

fn tick(tables: &mut super::tables::Tables) -> (bool, wire::VersionLedger) {
    match handle_for_test(tables, put_t(true)) {
        Rep::Notice { check, version } => (check, version),
        other => panic!("{other:?}"),
    }
}

#[test]
fn put_t_without_notice_starts_nothing() {
    let mut tables = empty_tables();
    assert_eq!(handle_for_test(&mut tables, put_t(false)), Rep::Ok);
    assert!(!take_version_fetch_for_test(&mut tables));
    let (check, _) = tick(&mut tables);
    assert!(check, "the slot is still free");
}

#[test]
fn first_tick_owns_the_slot_and_starts_one_version_fetch() {
    let mut tables = empty_tables();
    let (check, version) = tick(&mut tables);
    assert!(check);
    assert!(version.tried_at_ms.is_some());
    assert_eq!(version.latest, None);
    assert!(take_version_fetch_for_test(&mut tables));
    assert!(!take_version_fetch_for_test(&mut tables));

    let (check, _) = tick(&mut tables);
    assert!(!check, "one slot an hour");
    assert!(!take_version_fetch_for_test(&mut tables));
}

#[test]
fn slot_returns_hourly_and_version_fetch_daily() {
    let mut tables = empty_tables();
    tick(&mut tables);
    take_version_fetch_for_test(&mut tables);

    age_ledger_for_test(&mut tables, std::time::Duration::from_secs(3601));
    let (check, _) = tick(&mut tables);
    assert!(check);
    assert!(!take_version_fetch_for_test(&mut tables));

    age_ledger_for_test(&mut tables, std::time::Duration::from_secs(24 * 3600));
    let (check, _) = tick(&mut tables);
    assert!(check);
    assert!(take_version_fetch_for_test(&mut tables));
}

#[test]
fn version_read_starts_nothing() {
    let mut tables = empty_tables();
    assert_eq!(
        handle_for_test(&mut tables, Req::Version),
        Rep::Version(wire::VersionLedger::default())
    );
    assert!(!take_version_fetch_for_test(&mut tables));
}

#[test]
fn wire_roundtrip_notice_reply() {
    let rep = Rep::Notice {
        check: true,
        version: wire::VersionLedger {
            tried_at_ms: Some(1),
            latest: Some("0.20.0".into()),
            error: Some("timed out".into()),
        },
    };
    let frame = wire::encode_rep(&rep).unwrap();
    assert_eq!(wire::decode_rep(&frame), Some(rep));
}

#[test]
fn orphan_hub_exits_even_with_live_rows() {
    let mut live = empty_tables();
    put_secret(&mut live, 60_000);
    assert!(should_exit_for_test(&mut live, false, false));
}

#[test]
fn a_running_binary_is_its_own_image() {
    let snap = super::peer::snapshot_self().expect("self image");
    assert!(super::peer::image_is_current(snap.inode()));
    assert!(!super::peer::image_is_current((0, 0)));
}

/// Runs `serve_until_idle` on its own thread and returns a receiver that
/// fires when it returns.
fn spawn_idle_hub(idle: std::time::Duration) -> std::sync::mpsc::Receiver<()> {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = rt.block_on(super::server::serve_until_idle(
            idle,
            std::time::Duration::from_secs(60),
            std::future::pending(),
        ));
        let _ = done.send(());
    });
    finished
}

fn wait_for(path: &std::path::Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("{} never appeared", path.display());
}

#[test]
fn idle_empty_hub_exits_and_removes_its_socket() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        let lock = super::server::lock_path().unwrap();
        std::fs::write(&lock, b"").unwrap();
        let finished = spawn_idle_hub(std::time::Duration::from_millis(50));
        let sock = super::server::socket_path().unwrap();
        wait_for(&sock);
        finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("idle hub exits");
        assert!(!sock.exists());
        assert!(!lock.exists());
    });
}

#[test]
fn orphan_hub_keeps_the_socket_that_replaced_it() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        let lock = super::server::lock_path().unwrap();
        std::fs::write(&lock, b"").unwrap();
        let finished = spawn_idle_hub(std::time::Duration::from_millis(200));
        let sock = super::server::socket_path().unwrap();
        wait_for(&sock);
        // What a client does when it gives up on this hub and binds a new one.
        std::fs::remove_file(&sock).unwrap();
        std::fs::write(&sock, b"next hub").unwrap();
        finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("orphan hub exits");
        assert_eq!(std::fs::read(&sock).unwrap(), b"next hub");
        assert!(lock.exists(), "the next hub's lock stays");
    });
}

#[test]
fn a_silent_client_does_not_hold_the_hub() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        let finished = spawn_idle_hub(std::time::Duration::from_secs(30));
        let sock = super::server::socket_path().unwrap();
        wait_for(&sock);
        let silent = std::os::unix::net::UnixStream::connect(&sock).unwrap();
        let up = (0..300).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            super::client::probe().state == super::client::HubState::Up
        });
        assert!(up, "hub still serves while a client sends nothing");
        drop(silent);
        assert_eq!(super::client::stop(), super::client::HubStop::Stopped);
        finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("hub quits");
    });
}

#[test]
fn socket_and_lock_follow_the_image_token() {
    let token = super::peer::instance_token().unwrap();
    let sock = super::server::socket_path().unwrap();
    let lock = super::server::lock_path().unwrap();
    let want_sock = format!("hub-{token}.sock");
    let want_lock = format!("hub-{token}.lock");
    assert_eq!(
        sock.file_name().and_then(|n| n.to_str()),
        Some(want_sock.as_str())
    );
    assert_eq!(
        lock.file_name().and_then(|n| n.to_str()),
        Some(want_lock.as_str())
    );
}

#[test]
fn log_then_shell_ttl_body_roundtrips_hub() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let proj = root.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join("lade.yml"),
        ".:\n  - .:\n      log: true\n  - .:\n      ttl: 24h\n    SOCKET: sh://printf %s /tmp/app\n",
    )
    .unwrap();
    let cache_s = cache.to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache_s.as_str()))], || {
        let _server = std::thread::spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _ = rt.block_on(crate::hub::serve(std::future::pending()));
        });
        let sock = super::server::socket_path().unwrap();
        for _ in 0..80 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(sock.exists(), "hub socket");
        let config = crate::config::LadeFile::build(proj.clone()).unwrap();
        let patterned = config
            .collect_for_with_pattern("true", crate::config::Audience::Agent)
            .unwrap();
        let mut hydrated = std::collections::HashMap::new();
        hydrated.insert("SOCKET".into(), "/tmp/sock".into());
        crate::hub::Cache::open(&proj, config.walk_hash(), &None, &patterned)
            .store(&hydrated, &std::collections::HashSet::new());
        let hit = crate::hub::Cache::open(&proj, config.walk_hash(), &None, &patterned).lookup();
        assert_eq!(
            hit.values.get("SOCKET").map(String::as_str),
            Some("/tmp/sock"),
            "keys={:?} values={:?}",
            hit.keys,
            hit.values
        );
        assert_eq!(crate::hub::stop(), crate::hub::HubStop::Stopped);
    });
}

#[test]
fn user_map_shell_ttl_body_roundtrips_hub() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let proj = root.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join("lade.yml"),
        ".:\n  - .:\n      log: true\n  - .:\n      ttl: 24h\n    SOCKET:\n      owner: sh://printf %s /tmp/app\n",
    )
    .unwrap();
    let cache_s = cache.to_string_lossy().into_owned();
    let user = Some("owner".to_string());
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache_s.as_str()))], || {
        let _server = std::thread::spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _ = rt.block_on(crate::hub::serve(std::future::pending()));
        });
        let sock = super::server::socket_path().unwrap();
        for _ in 0..80 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(sock.exists(), "hub socket");
        let config = crate::config::LadeFile::build(proj.clone()).unwrap();
        let patterned = config
            .collect_for_with_pattern("true", crate::config::Audience::Agent)
            .unwrap();
        let mut hydrated = std::collections::HashMap::new();
        hydrated.insert("SOCKET".into(), "/tmp/sock".into());
        crate::hub::Cache::open(&proj, config.walk_hash(), &user, &patterned)
            .store(&hydrated, &std::collections::HashSet::new());
        let hit = crate::hub::Cache::open(&proj, config.walk_hash(), &user, &patterned).lookup();
        assert_eq!(
            hit.values.get("SOCKET").map(String::as_str),
            Some("/tmp/sock"),
            "keys={:?} values={:?}",
            hit.keys,
            hit.values
        );
        assert_eq!(crate::hub::stop(), crate::hub::HubStop::Stopped);
    });
}

#[test]
fn a_value_roundtrips_via_hub() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let proj = root.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join("lade.yaml"),
        "\"terraform .*\":\n  KEY: op://v/i/f\n",
    )
    .unwrap();
    let cache_s = cache.to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache_s.as_str()))], || {
        let _server = std::thread::spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _ = rt.block_on(crate::hub::serve(std::future::pending()));
        });
        let sock = super::server::socket_path().unwrap();
        for _ in 0..80 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(sock.exists(), "hub socket");
        let config = crate::config::LadeFile::build(proj.clone()).unwrap();
        let patterned = config
            .collect_for_with_pattern("terraform plan", crate::config::Audience::Human)
            .unwrap();
        let mut hydrated = std::collections::HashMap::new();
        hydrated.insert("KEY".into(), "secret-value".into());
        crate::hub::Cache::open(&proj, config.walk_hash(), &None, &patterned)
            .store(&hydrated, &std::collections::HashSet::new());
        let hit = crate::hub::Cache::open(&proj, config.walk_hash(), &None, &patterned).lookup();
        assert_eq!(
            hit.values.get("KEY").map(String::as_str),
            Some("secret-value")
        );
        let info = crate::hub::probe();
        assert_eq!(info.state, crate::hub::HubState::Up);
        assert_eq!(info.secrets, 1);
        assert_eq!(info.tickets, 0);
        assert!(info.pid.is_some());
        assert_eq!(crate::hub::stop(), crate::hub::HubStop::Stopped);
        let after = crate::hub::probe();
        assert_eq!(after.state, crate::hub::HubState::Down);
        assert!(!sock.exists(), "quit unlinks socket");
    });
}

#[test]
fn denied_hub_unlinks_and_fresh_hub_serves() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;

    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let proj = root.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join("lade.yaml"),
        "\"terraform .*\":\n  KEY: op://v/i/f\n",
    )
    .unwrap();
    let cache_s = cache.to_string_lossy().into_owned();
    let sock = cache.join(format!(
        "hub-{}.sock",
        super::peer::instance_token().unwrap()
    ));
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache_s.as_str()))], || {
        let stale = sock.clone();
        let _denied = std::thread::spawn(move || {
            let _ = std::fs::remove_file(&stale);
            let Ok(listener) = UnixListener::bind(&stale) else {
                return;
            };
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else {
                    break;
                };
                let mut header = [0u8; 8];
                if stream.read_exact(&mut header).is_err() {
                    continue;
                }
                let Some(n) = wire::header_len(&header) else {
                    continue;
                };
                let mut body = vec![0u8; n as usize];
                let _ = stream.read_exact(&mut body);
                let Some(frame) = wire::encode_rep(&Rep::Err(wire::ERR_DENIED)) else {
                    continue;
                };
                let _ = stream.write_all(&frame);
            }
        });
        for _ in 0..80 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(sock.exists(), "denied socket");
        let config = crate::config::LadeFile::build(proj.clone()).unwrap();
        let patterned = config
            .collect_for_with_pattern("terraform plan", crate::config::Audience::Human)
            .unwrap();
        let mut hydrated = std::collections::HashMap::new();
        hydrated.insert("KEY".into(), "after-upgrade".into());
        crate::hub::Cache::open(&proj, config.walk_hash(), &None, &patterned)
            .store(&hydrated, &std::collections::HashSet::new());
        assert!(
            !sock.exists() || UnixListener::bind(&sock).is_ok(),
            "stale socket still owned"
        );
        let _ = std::fs::remove_file(&sock);
        let _server = std::thread::spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _ = rt.block_on(crate::hub::serve(std::future::pending()));
        });
        for _ in 0..80 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(sock.exists(), "fresh hub socket");
        crate::hub::Cache::open(&proj, config.walk_hash(), &None, &patterned)
            .store(&hydrated, &std::collections::HashSet::new());
        let hit = crate::hub::Cache::open(&proj, config.walk_hash(), &None, &patterned).lookup();
        assert_eq!(
            hit.values.get("KEY").map(String::as_str),
            Some("after-upgrade")
        );
    });
}

fn fake_hub(sock: &std::path::Path, delay: Option<std::time::Duration>) {
    use std::io::{Read, Write};
    let listener = std::os::unix::net::UnixListener::bind(sock).unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else {
                break;
            };
            let mut header = [0u8; 8];
            if stream.read_exact(&mut header).is_err() {
                continue;
            }
            let Some(n) = wire::header_len(&header) else {
                continue;
            };
            let mut body = vec![0u8; n as usize];
            let _ = stream.read_exact(&mut body);
            let Some(delay) = delay else {
                held.push(stream);
                continue;
            };
            std::thread::sleep(delay);
            let rep = Rep::Stat {
                pid: 1,
                secrets: 0,
                tickets: 0,
                unguarded: None,
            };
            let _ = stream.write_all(&wire::encode_rep(&rep).unwrap());
        }
    });
}

/// A secret row key, every part named.
#[derive(Clone)]
struct Key<'a> {
    cwd: &'a str,
    path: &'a str,
    rule: &'a str,
    when: &'a str,
    walk: [u8; 32],
    user: &'a str,
}

const KEY: Key<'static> = Key {
    cwd: "/proj",
    path: "/proj/lade.yaml",
    rule: "t .*",
    when: "always",
    walk: [4u8; 32],
    user: "",
};

fn put_at(key: &Key, ttl_ms: u32, name: &str) -> Req {
    Req::Put {
        cwd: key.cwd.into(),
        path: key.path.into(),
        rule: key.rule.into(),
        when: key.when.into(),
        walk: key.walk,
        user: key.user.into(),
        ttl_ms,
        bindings: vec![(name.into(), b"v".to_vec())],
    }
}

fn get_at(key: &Key) -> Req {
    Req::Get {
        cwd: key.cwd.into(),
        path: key.path.into(),
        rule: key.rule.into(),
        when: key.when.into(),
        walk: key.walk,
        user: key.user.into(),
    }
}

/// `(name, ttl_left_ms)` of every listed row.
fn listed(tables: &mut super::tables::Tables) -> Vec<(String, u32)> {
    match handle_for_test(tables, Req::List) {
        Rep::Listing { rows, .. } => rows
            .into_iter()
            .map(|row| (row.name, row.ttl_left_ms))
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn left_of(tables: &mut super::tables::Tables, name: &str) -> Option<u32> {
    listed(tables)
        .into_iter()
        .find(|(listed, _)| listed == name)
        .map(|(_, left)| left)
}

fn set_window(tables: &mut super::tables::Tables, scope: &str, ttl: &str) {
    let req = Req::SetWindow {
        scope: scope.into(),
        ttl: Some(ttl.into()),
    };
    assert_eq!(handle_for_test(tables, req), Rep::Ok);
}

#[test]
fn window_leaves_eval_rows_alone() {
    let mut tables = empty_tables();
    let eval = Key {
        rule: "op://v/i/f",
        when: "eval",
        path: "",
        walk: [0; 32],
        ..KEY
    };
    handle_for_test(&mut tables, put_at(&eval, 60_000, "op://v/i/f"));
    handle_for_test(&mut tables, put_at(&KEY, 60_000, "AWS"));
    set_window(&mut tables, "/proj", "2h");
    assert!(left_of(&mut tables, "op://v/i/f").unwrap() <= 60_000);
    assert!(left_of(&mut tables, "AWS").unwrap() > 60_000);
}

fn window_at(tables: &mut super::tables::Tables, scope: &str) -> Option<String> {
    let req = Req::GetWindow {
        scope: scope.into(),
    };
    match handle_for_test(tables, req) {
        Rep::Window { ttl } => ttl,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_nearest_window_wins_and_a_tie_is_settled_by_path() {
    let mut tables = empty_tables();
    set_window(&mut tables, "/repo/b", "3h");
    set_window(&mut tables, "/repo/a", "2h");
    assert_eq!(window_at(&mut tables, "/repo").as_deref(), Some("2h"));
    set_window(&mut tables, "/repo/c/d", "4h");
    set_window(&mut tables, "/repo", "30m");
    assert_eq!(window_at(&mut tables, "/repo").as_deref(), Some("30m"));
    assert_eq!(window_at(&mut tables, "/repo/a/x").as_deref(), Some("2h"));
    assert_eq!(window_at(&mut tables, "/repo/c").as_deref(), Some("30m"));
    assert_eq!(window_at(&mut tables, "/repo/c/d/e").as_deref(), Some("4h"));
}

#[test]
fn a_window_restamps_only_the_rows_it_governs() {
    let mut tables = empty_tables();
    let sub = Key {
        cwd: "/proj/sub/x",
        ..KEY
    };
    handle_for_test(&mut tables, put_at(&sub, 60_000, "SUB"));
    handle_for_test(&mut tables, put_at(&KEY, 60_000, "ROOT"));
    set_window(&mut tables, "/proj/sub", "2h");
    set_window(&mut tables, "/proj", "8h");
    let sub_left = left_of(&mut tables, "SUB").unwrap();
    assert!(sub_left > 60_000 && sub_left <= 2 * 3_600_000, "{sub_left}");
    assert!(left_of(&mut tables, "ROOT").unwrap() > 2 * 3_600_000);
}

#[test]
fn get_does_not_refresh_the_clock() {
    let mut tables = empty_tables();
    handle_for_test(&mut tables, put_at(&KEY, 60_000, "AWS"));
    std::thread::sleep(std::time::Duration::from_millis(20));
    let before = left_of(&mut tables, "AWS").unwrap();
    assert!(before < 60_000, "{before}");
    assert!(matches!(
        handle_for_test(&mut tables, get_at(&KEY)),
        Rep::Hit { .. }
    ));
    assert!(left_of(&mut tables, "AWS").unwrap() <= before);
}

#[test]
fn expired_row_is_a_miss() {
    let mut tables = empty_tables();
    handle_for_test(&mut tables, put_at(&KEY, 1, "AWS"));
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(handle_for_test(&mut tables, get_at(&KEY)), Rep::Miss);
    assert!(listed(&mut tables).is_empty());
}

#[test]
fn each_key_part_separates_rows() {
    let others = [
        Key {
            cwd: "/other",
            ..KEY
        },
        Key {
            path: "/proj/sub/lade.yaml",
            ..KEY
        },
        Key {
            rule: "u .*",
            ..KEY
        },
        Key {
            when: "agent",
            ..KEY
        },
        Key {
            walk: [5u8; 32],
            ..KEY
        },
        Key {
            user: "alice",
            ..KEY
        },
    ];
    let mut tables = empty_tables();
    handle_for_test(&mut tables, put_at(&KEY, 60_000, "AWS"));
    assert!(matches!(
        handle_for_test(&mut tables, get_at(&KEY)),
        Rep::Hit { .. }
    ));
    for other in &others {
        assert_eq!(
            handle_for_test(&mut tables, get_at(other)),
            Rep::Miss,
            "{} {} {} {} {}",
            other.cwd,
            other.path,
            other.rule,
            other.when,
            other.user
        );
    }
}

#[test]
fn window_restamp_obeys_max_cache() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    write_config(&config, r#"{"max_cache":"1m"}"#);
    let mut tables = super::tables::tables_with_config(config.clone());
    handle_for_test(&mut tables, put_at(&KEY, 30_000, "AWS"));
    set_window(&mut tables, "/proj", "2h");
    let left = left_of(&mut tables, "AWS").unwrap();
    assert!(left > 30_000 && left <= 60_000, "{left}");
}

#[test]
fn unreadable_config_keeps_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    write_config(&config, r#"{"max_cache":"2h"}"#);
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_to_string(&config).is_ok() {
        eprintln!("running as a user who reads a mode 000 file: skipped");
        return;
    }
    let mut tables = super::tables::tables_with_config(config);
    assert_eq!(
        handle_for_test(&mut tables, put_at(&KEY, 60_000, "AWS")),
        Rep::Ok
    );
    assert!(listed(&mut tables).is_empty());
}

#[test]
fn missing_config_caps_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut tables = super::tables::tables_with_config(dir.path().join("config.json"));
    handle_for_test(&mut tables, put_at(&KEY, 3_600_000, "AWS"));
    assert!(left_of(&mut tables, "AWS").unwrap() > 60 * 60_000 - 1_000);
}

#[test]
fn max_cache_edit_applies_without_restart() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    write_config(&config, r#"{"max_cache":"1h"}"#);
    let mut tables = super::tables::tables_with_config(config.clone());
    handle_for_test(&mut tables, put_at(&KEY, 60_000, "AWS"));
    write_config(&config, r#"{"max_cache":"0"}"#);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !listed(&mut tables).is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the watcher did not apply the edit"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn full_table_evicts_oldest_insert() {
    let mut tables = empty_tables();
    let cwds: Vec<String> = (0..257).map(|i| format!("/c{i}")).collect();
    for cwd in &cwds {
        handle_for_test(&mut tables, put_at(&Key { cwd, ..KEY }, 60_000, "AWS"));
    }
    assert_eq!(
        handle_for_test(
            &mut tables,
            get_at(&Key {
                cwd: &cwds[0],
                ..KEY
            })
        ),
        Rep::Miss
    );
    for cwd in &cwds[1..] {
        assert!(matches!(
            handle_for_test(&mut tables, get_at(&Key { cwd, ..KEY })),
            Rep::Hit { .. }
        ));
    }
}

/// Starts a hub in this thread's `LADE_CACHE_DIR`, runs `f`, and stops it.
fn with_live_hub(f: impl FnOnce()) {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        let _server = std::thread::spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _ = rt.block_on(crate::hub::serve(std::future::pending()));
        });
        let sock = super::server::socket_path().unwrap();
        for _ in 0..80 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(sock.exists(), "hub socket");
        f();
        assert_eq!(crate::hub::stop(), crate::hub::HubStop::Stopped);
    });
}

#[test]
fn a_partial_hit_does_not_restart_the_clock_of_served_values() {
    let proj = tempfile::tempdir().unwrap();
    let proj = proj.path().to_path_buf();
    std::fs::write(
        proj.join("lade.yaml"),
        "\"terraform .*\":\n  A: op://v/i/a\n  B: op://v/i/b\n",
    )
    .unwrap();
    with_live_hub(|| {
        let config = crate::config::LadeFile::build(proj.clone()).unwrap();
        let patterned = config
            .collect_for_with_pattern("terraform plan", crate::config::Audience::Human)
            .unwrap();
        let cache = || crate::hub::Cache::open(&proj, config.walk_hash(), &None, &patterned);
        let left = |name: &str| {
            crate::hub::listing()
                .rows
                .into_iter()
                .find(|row| row.name == name)
                .map(|row| row.ttl_left_ms)
        };
        let fetched: std::collections::HashMap<String, String> =
            [("A".into(), "a1".into()), ("B".into(), "b1".into())].into();
        cache().store(&fetched, &std::collections::HashSet::new());
        crate::hub::forget_names(&["B".to_string()]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let before = left("A").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(50));
        let hit = cache().lookup();
        assert_eq!(hit.keys, std::collections::HashSet::from(["A".to_string()]));
        let mut hydrated = hit.values.clone();
        hydrated.insert("B".into(), "b2".into());
        cache().store(&hydrated, &hit.keys);
        assert!(left("A").unwrap() <= before, "A's clock restarted");
        assert_eq!(left("B"), None);
    });
}

#[test]
fn a_slow_hub_is_not_stale_and_a_wedged_one_is() {
    for (delay, stale) in [
        (Some(std::time::Duration::from_millis(100)), false),
        (None, true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().to_string_lossy().into_owned();
        temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
            fake_hub(&super::server::socket_path().unwrap(), delay);
            assert_eq!(super::transport::is_stale(&None, true), stale);
        });
    }
}

/// A hub that reads each request and closes the connection with no reply.
/// With `answer_ping`, it still answers a `Ping`: a hub too busy for one
/// request, not a wedged one.
fn dropping_hub(sock: &std::path::Path, answer_ping: bool) {
    use std::io::{Read, Write};
    let listener = std::os::unix::net::UnixListener::bind(sock).unwrap();
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            let Ok(mut stream) = incoming else {
                break;
            };
            let mut header = [0u8; 8];
            if stream.read_exact(&mut header).is_err() {
                continue;
            }
            let Some(n) = wire::header_len(&header) else {
                continue;
            };
            let mut frame = header.to_vec();
            frame.resize(8 + n as usize, 0);
            if stream.read_exact(&mut frame[8..]).is_err() {
                continue;
            }
            let ping = matches!(wire::decode_in_req(&frame), Some(InReq::Body(body)) if matches!(*body, Req::Ping));
            if answer_ping && ping {
                let rep = Rep::Stat {
                    pid: 1,
                    secrets: 0,
                    tickets: 0,
                    unguarded: None,
                };
                let _ = stream.write_all(&wire::encode_rep(&rep).unwrap());
            }
        }
    });
}

#[test]
fn a_hub_that_drops_a_request_is_stale_only_when_it_answers_no_ping() {
    for (answer_ping, stale) in [(true, false), (false, true)] {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().to_string_lossy().into_owned();
        temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
            dropping_hub(&super::server::socket_path().unwrap(), answer_ping);
            assert_eq!(super::transport::rpc_existing(Req::List), None);
            assert_eq!(super::transport::is_stale(&None, true), stale);
        });
    }
}

#[test]
fn an_unguarded_hub_caches_nothing() {
    let mut tables = super::tables::unguarded_tables();
    assert_eq!(
        handle_for_test(&mut tables, put_at(&KEY, 60_000, "AWS")),
        Rep::Ok
    );
    assert_eq!(handle_for_test(&mut tables, get_at(&KEY)), Rep::Miss);
    assert!(listed(&mut tables).is_empty());
    match handle_for_test(&mut tables, Req::Ping) {
        Rep::Stat {
            secrets: 0,
            unguarded: Some(why),
            ..
        } => assert!(why.contains("not dumpable"), "{why}"),
        other => panic!("{other:?}"),
    }
}

fn hub_sockets(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("hub-") && name.ends_with(".sock"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn an_unreadable_image_reaches_no_hub() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        super::peer::NO_IMAGE_FOR_TEST.with(|off| off.set(true));
        assert!(super::server::socket_path().is_none());
        assert_eq!(super::transport::rpc(Req::Ping), None);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let error = rt
            .block_on(super::server::serve_until_idle(
                std::time::Duration::from_millis(50),
                std::time::Duration::from_secs(60),
                std::future::pending(),
            ))
            .unwrap_err();
        assert!(
            error.to_string().contains("cannot read its own executable"),
            "{error:#}"
        );
        assert_eq!(hub_sockets(root.path()), Vec::<String>::new());
    });
}

#[test]
fn hub_socket_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        let finished = spawn_idle_hub(std::time::Duration::from_secs(30));
        let sock = super::server::socket_path().unwrap();
        wait_for(&sock);
        let mode = std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
        assert_eq!(super::client::stop(), super::client::HubStop::Stopped);
        finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("hub quits");
        assert_eq!(mode, 0o600, "{mode:o}");
    });
}

#[test]
fn a_busy_orphan_still_exits() {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        let reach = std::time::Duration::from_millis(200);
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _ = rt.block_on(super::server::serve_until_idle(
                std::time::Duration::from_secs(3600),
                reach,
                std::future::pending(),
            ));
            let _ = done.send(());
        });
        let sock = super::server::socket_path().unwrap();
        wait_for(&sock);
        // An old client still reaches the hub through this second name once
        // the socket path holds another file.
        let alias = root.path().join("old-client.sock");
        std::fs::hard_link(&sock, &alias).unwrap();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let pinging = std::sync::Arc::clone(&stop);
        let pinger = std::thread::spawn(move || {
            let ping = wire::encode_req(&Req::Ping).unwrap();
            let mut answered = 0u32;
            while !pinging.load(Ordering::Relaxed) {
                if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&alias) {
                    let mut reply = Vec::new();
                    if stream.write_all(&ping).is_ok()
                        && stream.read_to_end(&mut reply).is_ok()
                        && !reply.is_empty()
                    {
                        answered += 1;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            answered
        });
        std::thread::sleep(reach / 2);
        std::fs::remove_file(&sock).unwrap();
        std::fs::write(&sock, b"next hub").unwrap();
        let exited = finished.recv_timeout(std::time::Duration::from_secs(2));
        stop.store(true, Ordering::Relaxed);
        let answered = pinger.join().unwrap();
        exited.expect("a busy orphan exits");
        assert!(answered > 5, "the hub was not busy: {answered} replies");
        assert_eq!(std::fs::read(&sock).unwrap(), b"next hub");
    });
}

#[test]
fn a_release_client_starts_its_own_executable() {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::transport::hub_exe_from;
    let lade = PathBuf::from("/opt/bin/lade");
    let planted = Some(OsString::from("/tmp/planted"));
    assert_eq!(
        hub_exe_from(lade.clone(), planted.clone(), false),
        Some(lade.clone())
    );
    assert_eq!(
        hub_exe_from(PathBuf::from("/opt/bin/lade-other"), planted.clone(), false),
        None
    );
    assert_eq!(hub_exe_from(lade.clone(), None, true), Some(lade));
    assert_eq!(
        hub_exe_from(PathBuf::from("/target/debug/deps/harness"), planted, true),
        Some(PathBuf::from("/tmp/planted"))
    );
}

fn terraform_project() -> tempfile::TempDir {
    let proj = tempfile::tempdir().unwrap();
    std::fs::write(
        proj.path().join("lade.yaml"),
        "\"terraform .*\":\n  KEY: op://v/i/f\n",
    )
    .unwrap();
    proj
}

fn terraform_cache(proj: &std::path::Path) -> crate::hub::Cache {
    let config = crate::config::LadeFile::build(proj.to_path_buf()).unwrap();
    let patterned = config
        .collect_for_with_pattern("terraform plan", crate::config::Audience::Human)
        .unwrap();
    crate::hub::Cache::open(proj, config.walk_hash(), &None, &patterned)
}

#[test]
fn with_the_guard_off_a_command_never_calls_the_hub_for_secrets() {
    let proj = terraform_project();
    let proj = proj.path();
    let hydrated: std::collections::HashMap<String, String> = [("KEY".into(), "v".into())].into();
    let none = std::collections::HashSet::new();
    with_live_hub(|| {
        super::guard::GUARD_OFF_FOR_TEST.with(|off| off.set(true));
        terraform_cache(proj).store(&hydrated, &none);
        assert!(
            crate::hub::listing().rows.is_empty(),
            "a Put reached the hub"
        );

        super::guard::GUARD_OFF_FOR_TEST.with(|off| off.set(false));
        terraform_cache(proj).store(&hydrated, &none);
        assert_eq!(crate::hub::listing().rows.len(), 1);

        super::guard::GUARD_OFF_FOR_TEST.with(|off| off.set(true));
        assert!(
            terraform_cache(proj).lookup().values.is_empty(),
            "a Get reached the hub"
        );
    });
    // No hub for it to start either: `Cache::open` plans nothing.
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        super::guard::GUARD_OFF_FOR_TEST.with(|off| off.set(true));
        let opened = terraform_cache(proj);
        opened.store(&hydrated, &none);
        assert!(opened.lookup().values.is_empty());
        assert_eq!(hub_sockets(root.path()), Vec::<String>::new());
    });
}

#[test]
fn header_len_refuses_an_empty_or_oversized_body() {
    let header = |n: u32| {
        let mut header = [0u8; 8];
        header[..4].copy_from_slice(wire::MAGIC);
        header[4..].copy_from_slice(&n.to_le_bytes());
        header
    };
    assert_eq!(wire::header_len(&header(0)), None);
    assert_eq!(wire::header_len(&header(wire::MAX_BODY + 1)), None);
    assert_eq!(
        wire::header_len(&header(wire::MAX_BODY)),
        Some(wire::MAX_BODY)
    );
}

#[test]
fn a_hub_closes_a_bad_frame_with_no_reply() {
    use std::io::{Read, Write};
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        let finished = spawn_idle_hub(std::time::Duration::from_secs(30));
        let sock = super::server::socket_path().unwrap();
        wait_for(&sock);
        let mut empty = *b"LDH1\0\0\0\0";
        let mut oversized = *b"LDH1\0\0\0\0";
        oversized[4..].copy_from_slice(&(wire::MAX_BODY + 1).to_le_bytes());
        let mut magic = *b"XXXX\x01\0\0\0";
        for header in [&mut empty, &mut oversized, &mut magic] {
            let mut stream = std::os::unix::net::UnixStream::connect(&sock).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            stream.write_all(header.as_slice()).unwrap();
            let mut reply = Vec::new();
            stream.read_to_end(&mut reply).expect("the hub closes it");
            assert!(reply.is_empty(), "{reply:?}");
        }
        assert_eq!(super::client::probe().state, super::client::HubState::Up);
        assert_eq!(super::client::stop(), super::client::HubStop::Stopped);
        finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("hub quits");
    });
}

#[test]
fn a_slow_hub_makes_a_lookup_miss_in_time() {
    let proj = terraform_project();
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    temp_env::with_vars([("LADE_CACHE_DIR", Some(cache.as_str()))], || {
        fake_hub(
            &super::server::socket_path().unwrap(),
            Some(std::time::Duration::from_millis(100)),
        );
        let started = std::time::Instant::now();
        let hit = terraform_cache(proj.path()).lookup();
        let took = started.elapsed();
        assert!(hit.values.is_empty());
        assert!(took < std::time::Duration::from_millis(1500), "{took:?}");
    });
}

#[test]
fn a_lookup_misses_in_time_while_another_client_holds_the_start_lock() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().to_string_lossy().into_owned();
    let missing_hub = root.path().join("no-such-lade");
    temp_env::with_vars(
        [
            ("LADE_CACHE_DIR", Some(cache.as_str())),
            ("CARGO_BIN_EXE_lade", Some(missing_hub.to_str().unwrap())),
        ],
        || {
            let file = std::fs::File::create(super::server::lock_path().unwrap()).unwrap();
            let _held = nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusive).unwrap();
            let (done, wait) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let rep = super::transport::rpc(Req::Ping);
                let _ = done.send((rep, started.elapsed()));
            });
            let (rep, took) = wait
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("the lookup waited on the start lock");
            assert_eq!(rep, None);
            assert!(took < std::time::Duration::from_millis(500), "{took:?}");
        },
    );
}

#[test]
fn a_ticket_goes_after_an_hour_and_the_oldest_past_128() {
    let put = |t: [u8; 4]| Req::PutT {
        t,
        ticket: b"{}".to_vec(),
        notice: false,
    };
    let held = |tables: &mut super::tables::Tables, t: [u8; 4]| {
        matches!(handle_for_test(tables, Req::GetT { t }), Rep::Ticket { .. })
    };
    let mut tables = empty_tables();
    assert_eq!(handle_for_test(&mut tables, put(*b"aaaa")), Rep::Ok);
    super::tables::advance_clock_for_test(std::time::Duration::from_secs(3599));
    assert!(held(&mut tables, *b"aaaa"));
    super::tables::advance_clock_for_test(std::time::Duration::from_secs(1));
    assert!(!held(&mut tables, *b"aaaa"), "an hour-old ticket stays");

    let ids: Vec<[u8; 4]> = (0..129u32)
        .map(|n| {
            let text = format!("{n:04}");
            text.as_bytes().try_into().unwrap()
        })
        .collect();
    for id in &ids {
        assert_eq!(handle_for_test(&mut tables, put(*id)), Rep::Ok);
        super::tables::advance_clock_for_test(std::time::Duration::from_millis(1));
    }
    assert!(!held(&mut tables, ids[0]), "the oldest stays past 128");
    assert!(ids[1..].iter().all(|id| held(&mut tables, *id)));
}

#[test]
fn a_failed_fetch_keeps_the_last_known_tag() {
    let mut ledger = wire::VersionLedger {
        tried_at_ms: Some(1),
        latest: Some("0.20.0".to_string()),
        error: None,
    };
    super::release::record_fetch(&mut ledger, Err(anyhow::anyhow!("offline")));
    assert_eq!(ledger.latest.as_deref(), Some("0.20.0"));
    assert_eq!(ledger.error.as_deref(), Some("offline"));
    super::release::record_fetch(&mut ledger, Ok("0.21.0".to_string()));
    assert_eq!(ledger.latest.as_deref(), Some("0.21.0"));
    assert_eq!(ledger.error, None);
}
