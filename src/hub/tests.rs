use super::crypto::{aad, open, seal};
use super::server::{empty_tables, handle_for_test};
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
    assert!(matches!(wire::decode_in_req(&frame), Some(InReq::Body(got)) if got == req));
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
                ticket: b"{}".to_vec()
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
                ticket: b"{}".to_vec()
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
        }
    );
    assert_eq!(handle_for_test(&mut tables, Req::Quit), Rep::Ok);
}

#[test]
fn socket_and_lock_follow_the_image_token() {
    let token = super::peer::instance_token();
    let sock = super::server::socket_path();
    let lock = super::server::lock_path();
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
fn probe_off_when_daemon_disabled() {
    temp_env::with_var("LADE_DAEMON", Some("off"), || {
        let info = crate::hub::probe();
        assert_eq!(info.state, crate::hub::HubState::Off);
        assert_eq!(info.pid, None);
        assert_eq!(info.secrets, 0);
        assert_eq!(info.tickets, 0);
    });
}

#[test]
fn daemon_off_skips_lookup() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        "\"echo\":\n  KEY: op://v/i/f\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("LADE_DAEMON", Some("off")),
            ("LADE_WRAP_KEY", Some("ab".repeat(32).as_str())),
        ],
        || {
            let config = crate::config::LadeFile::build(dir.path().to_path_buf()).unwrap();
            let patterned =
                config.collect_for_with_pattern("echo hi", crate::config::Audience::Human);
            let hit = crate::hub::lookup(dir.path(), config.walk_hash(), &None, &patterned);
            assert!(hit.values.is_empty());
        },
    );
}

#[test]
fn wrap_key_roundtrip_via_hub() {
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
    let wrap = "cd".repeat(32);
    let cache_s = cache.to_string_lossy().into_owned();
    temp_env::with_vars(
        [
            ("LADE_CACHE_DIR", Some(cache_s.as_str())),
            ("LADE_WRAP_KEY", Some(wrap.as_str())),
            ("LADE_DAEMON", None),
        ],
        || {
            let _server = std::thread::spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let _ = rt.block_on(crate::hub::serve());
            });
            let sock = super::server::socket_path();
            for _ in 0..80 {
                if sock.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(sock.exists(), "hub socket");
            let config = crate::config::LadeFile::build(proj.clone()).unwrap();
            let patterned =
                config.collect_for_with_pattern("terraform plan", crate::config::Audience::Human);
            let mut hydrated = std::collections::HashMap::new();
            hydrated.insert("KEY".into(), "secret-value".into());
            crate::hub::store(
                &proj,
                config.walk_hash(),
                &None,
                &patterned,
                &hydrated,
                &std::collections::HashSet::new(),
            );
            let hit = crate::hub::lookup(&proj, config.walk_hash(), &None, &patterned);
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
        },
    );
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
    let wrap = "ef".repeat(32);
    let cache_s = cache.to_string_lossy().into_owned();
    let sock = cache.join(format!("hub-{}.sock", super::peer::instance_token()));
    temp_env::with_vars(
        [
            ("LADE_CACHE_DIR", Some(cache_s.as_str())),
            ("LADE_WRAP_KEY", Some(wrap.as_str())),
            ("LADE_DAEMON", None),
        ],
        || {
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
            let patterned =
                config.collect_for_with_pattern("terraform plan", crate::config::Audience::Human);
            let mut hydrated = std::collections::HashMap::new();
            hydrated.insert("KEY".into(), "after-upgrade".into());
            crate::hub::store(
                &proj,
                config.walk_hash(),
                &None,
                &patterned,
                &hydrated,
                &std::collections::HashSet::new(),
            );
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
                let _ = rt.block_on(crate::hub::serve());
            });
            for _ in 0..80 {
                if sock.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(sock.exists(), "fresh hub socket");
            crate::hub::store(
                &proj,
                config.walk_hash(),
                &None,
                &patterned,
                &hydrated,
                &std::collections::HashSet::new(),
            );
            let hit = crate::hub::lookup(&proj, config.walk_hash(), &None, &patterned);
            assert_eq!(
                hit.values.get("KEY").map(String::as_str),
                Some("after-upgrade")
            );
        },
    );
}

#[test]
fn decrypt_wrong_aad_is_none() {
    let key = [2u8; 32];
    let walk = [4u8; 32];
    let good = aad("/c", "/c/lade.yaml", "t", "always", &walk, "");
    let blob = seal(&key, &good, b"secret").unwrap();
    let bad = aad("/c", "/c/lade.yaml", "t", "human", &walk, "");
    assert!(open(&key, &bad, &blob).is_none());
}
