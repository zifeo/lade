mod common;
use common::{
    SECRET, filter_log, init_git, inject, lade_user, log_rows, share_pack, stored_command,
    write_yml,
};
use tempfile::tempdir;

fn tar_members(pack: &std::path::Path) -> Vec<String> {
    let out = std::process::Command::new("tar")
        .args(["-tzf", pack.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "tar -tzf failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn live_row_count(home: &std::path::Path) -> i64 {
    let db = home.join("events.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap()
}

fn looks_like_window_pack(name: &str) -> bool {
    let Some(body) = name
        .strip_prefix("lade-alice-")
        .and_then(|s| s.strip_suffix(".tar.gz"))
    else {
        return false;
    };
    let Some((from, to)) = body.split_once('-') else {
        return false;
    };
    compact_utc_label(from) && compact_utc_label(to)
}

fn compact_utc_label(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() == 16
        && bytes[8] == b'T'
        && bytes[15] == b'Z'
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'T' || b == b'Z')
}

#[test]
fn share_writes_pack_in_cwd() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let before = live_row_count(home.path());
    let assert = lade_user(home.path())
        .current_dir(dir.path())
        .args(["log", "share", "--since", "1d"])
        .assert()
        .success();
    let out = assert.get_output();
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let pack = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("lade-alice-") && n.ends_with(".tar.gz"))
        })
        .expect("pack in cwd");
    assert!(stderr.contains(pack.to_str().unwrap()), "{stderr}");
    let name = pack.file_name().unwrap().to_str().unwrap();
    assert!(
        looks_like_window_pack(name),
        "filename should be lade-$USER-$FROM-$TO.tar.gz, got {name}"
    );
    let members = tar_members(&pack);
    assert_eq!(members, vec!["manifest.json", "events.db"]);
    assert_eq!(live_row_count(home.path()), before);
}

#[test]
fn share_does_not_overwrite() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    lade_user(home.path())
        .current_dir(dir.path())
        .args([
            "log",
            "share",
            "-o",
            pack.to_str().unwrap(),
            "--since",
            "1d",
        ])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("refusing to overwrite"));
}

#[test]
fn share_refuses_agent_audience() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    lade_user(home.path())
        .current_dir(dir.path())
        .env("CURSOR_AGENT", "1")
        .args(["log", "share", "--since", "1d"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("agent sessions"));
}

#[test]
fn share_redacts_actor_and_repo_basename() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    init_git(dir.path());
    write_yml(
        dir.path(),
        "\"^echo \":\n  API_TOKEN: tok_example_0000000001\n",
    );
    inject(home.path(), dir.path(), &["echo", SECRET]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    let rows = filter_log(
        home.path(),
        dir.path(),
        &["--source", pack.to_str().unwrap(), "--all"],
    );
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["actor"].is_null());
    let repo = rows[0]["repo"].as_str().unwrap();
    assert!(!repo.contains('/'), "{repo}");
    assert_eq!(repo, dir.path().file_name().unwrap().to_str().unwrap());
    if let Some(file) = rows[0]["matches"][0]["file"].as_str() {
        assert!(!file.contains('/'), "{file}");
    }
    let scoped = filter_log(
        home.path(),
        dir.path(),
        &["--source", pack.to_str().unwrap()],
    );
    assert_eq!(scoped.len(), 1);
}

#[test]
fn share_drops_empty_command_rows() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo \":\n  API_TOKEN: API\n");
    inject(home.path(), dir.path(), &["echo", "API"]);
    assert_eq!(stored_command(home.path(), dir.path()), "");
    let id = log_rows(home.path(), dir.path())[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let pack = share_pack(home.path(), dir.path(), &["--all"]);
    let packed = filter_log(
        home.path(),
        dir.path(),
        &["--source", pack.to_str().unwrap(), "--all"],
    );
    assert!(
        packed.iter().all(|r| r["id"] != id),
        "empty command id should not be packed: {packed:?}"
    );
}

#[test]
fn share_default_window_skips_old_rows() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "ancient"]);
    common::backdate_all(home.path(), "2020-01-01T00:00:00.000Z");
    let pack = share_pack(home.path(), dir.path(), &[]);
    let packed = filter_log(
        home.path(),
        dir.path(),
        &["--source", pack.to_str().unwrap(), "--limit", "10"],
    );
    assert!(packed.is_empty(), "{packed:?}");
}

#[test]
fn share_output_writes_only_there() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let custom = dir.path().join("out").join("custom.tar.gz");
    lade_user(home.path())
        .current_dir(dir.path())
        .args([
            "log",
            "share",
            "--since",
            "1d",
            "-o",
            custom.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(custom.is_file());
    let defaults = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("lade-alice-") && n.ends_with(".tar.gz"))
        })
        .count();
    assert_eq!(defaults, 0);
}

#[test]
fn share_and_prune_reject_source() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    lade_user(home.path())
        .current_dir(dir.path())
        .args([
            "log",
            "share",
            "--source",
            pack.to_str().unwrap(),
            "--since",
            "1d",
        ])
        .assert()
        .code(1)
        .stderr(predicates::str::contains(
            "--source cannot be used with share",
        ));
    lade_user(home.path())
        .current_dir(dir.path())
        .args([
            "log",
            "prune",
            "--keep",
            "1d",
            "--source",
            pack.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(predicates::str::contains(
            "--source cannot be used with prune",
        ));
}
