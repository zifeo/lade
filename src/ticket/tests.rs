use super::*;
use serde_json::json;

fn sample_pre_event() -> PreEvent {
    PreEvent {
        command: "npm test".to_string(),
        cwd: PathBuf::from("/tmp/proj"),
        via: "pretool".to_string(),
        audience: "agent".to_string(),
        actor: Some("alice".to_string()),
        log: true,
        disclaimers: vec!["warn".to_string()],
        secrets: vec![TicketSecret {
            key: "API_KEY".to_string(),
            source: "op://vault/item/field".to_string(),
            private: true,
            output: None,
            cwd: PathBuf::from("/tmp/proj"),
        }],
        network: vec![TicketNetwork {
            key: "db".to_string(),
            uri: "k8s://cluster/ns/svc".to_string(),
        }],
        matches: json!([]),
        op_sa: None,
        agent: json!({"harness": "cursor"}),
        network_pids: Vec::new(),
        pending: false,
    }
}

#[test]
fn is_id_accepts_valid_and_rejects_invalid() {
    assert!(is_id("x7Km"));
    assert!(!is_id("inject"));
    assert!(!is_id("eval"));
    assert!(!is_id("abc"));
    assert!(!is_id("x7K_"));
}

#[test]
fn peel_pretool_equals_form() {
    let (id, argv) = peel_pretool(vec![
        OsString::from("lade"),
        OsString::from("--pretool=x7Km"),
        OsString::from("echo"),
    ]);
    assert_eq!(id.as_deref(), Some("x7Km"));
    assert_eq!(
        argv,
        vec![
            OsString::from("lade"),
            OsString::from("--pretool"),
            OsString::from("echo"),
        ]
    );
}

#[test]
fn peel_pretool_space_form() {
    fs::create_dir_all(dir()).unwrap();
    fs::write(path("x7Km"), "{}").unwrap();
    let (id, argv) = peel_pretool(vec![
        OsString::from("lade"),
        OsString::from("--pretool"),
        OsString::from("x7Km"),
        OsString::from("echo"),
    ]);
    unlink("x7Km").unwrap();
    assert_eq!(id.as_deref(), Some("x7Km"));
    assert_eq!(
        argv,
        vec![
            OsString::from("lade"),
            OsString::from("--pretool"),
            OsString::from("echo"),
        ]
    );
}

#[test]
fn peel_pretool_does_not_consume_echo_without_file() {
    let _ = unlink("echo");
    let (id, argv) = peel_pretool(vec![
        OsString::from("lade"),
        OsString::from("--pretool"),
        OsString::from("echo"),
        OsString::from("hi"),
    ]);
    assert!(id.is_none());
    assert_eq!(
        argv,
        vec![
            OsString::from("lade"),
            OsString::from("--pretool"),
            OsString::from("echo"),
            OsString::from("hi"),
        ]
    );
}

#[test]
fn peel_pretool_does_not_consume_inject() {
    let (id, argv) = peel_pretool(vec![
        OsString::from("lade"),
        OsString::from("--pretool"),
        OsString::from("inject"),
        OsString::from("echo"),
    ]);
    assert!(id.is_none());
    assert_eq!(
        argv,
        vec![
            OsString::from("lade"),
            OsString::from("--pretool"),
            OsString::from("inject"),
            OsString::from("echo"),
        ]
    );
}

#[test]
fn write_twice_in_one_ms_does_not_spin() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_str().unwrap();
    temp_env::with_var("LADE_TICKET_DIR", Some(path), || {
        let pre = sample_pre_event();
        let mut ids = std::collections::HashSet::new();
        for _ in 0..16 {
            assert!(ids.insert(write(&pre).expect("write ticket")));
        }
        for id in ids {
            unlink(&id).unwrap();
        }
    });
}

#[test]
fn write_read_unlink_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_str().unwrap();
    temp_env::with_var("LADE_TICKET_DIR", Some(path), || {
        write_read_unlink_roundtrip_inner();
    });
}

fn write_read_unlink_roundtrip_inner() {
    let pre = sample_pre_event();
    let id = write(&pre).expect("write ticket");
    assert!(is_id(&id));
    let read_back = read(&id).expect("read ticket");
    assert_eq!(read_back, pre);
    unlink(&id).expect("unlink ticket");
    assert!(!path(&id).exists());
    unlink(&id).expect("unlink missing is ok");
}

#[test]
fn replace_updates_same_id() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_str().unwrap();
    temp_env::with_var("LADE_TICKET_DIR", Some(dir), || {
        let mut pre = sample_pre_event();
        let id = write(&pre).unwrap();
        pre.pending = true;
        pre.network_pids = vec![42];
        replace(&id, &pre).unwrap();
        let read_back = read(&id).unwrap();
        assert!(read_back.pending);
        assert_eq!(read_back.network_pids, vec![42]);
        unlink(&id).unwrap();
    });
}
