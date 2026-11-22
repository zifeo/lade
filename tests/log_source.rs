mod common;
use common::{
    backdate_all, filter_log, inject, lade, lade_user, leftover_src_dirs_in, row_line, share_pack,
    write_yml,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn source_pack_returns_shared_rows() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "from-pack"]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    let live = common::log_rows(home.path(), dir.path());
    let from_pack = filter_log(
        home.path(),
        dir.path(),
        &["--source", pack.to_str().unwrap()],
    );
    assert_eq!(from_pack.len(), live.as_array().unwrap().len());
    assert_eq!(from_pack[0]["id"], live[0]["id"]);
}

#[test]
fn source_local_and_pack_unions_without_duplicates() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "union"]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    let local = filter_log(home.path(), dir.path(), &["--source", "local"]);
    let both = filter_log(
        home.path(),
        dir.path(),
        &["--source", "local", "--source", pack.to_str().unwrap()],
    );
    assert_eq!(both.len(), local.len());
}

#[test]
fn source_directory_reads_all_packs() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "one"]);
    let pack_a = share_pack(home.path(), dir.path(), &[]);
    std::fs::rename(&pack_a, dir.path().join("a.tar.gz")).unwrap();
    inject(home.path(), dir.path(), &["echo", "two"]);
    let pack_b = share_pack(home.path(), dir.path(), &[]);
    std::fs::rename(&pack_b, dir.path().join("b.tar.gz")).unwrap();
    let packs_dir = dir.path().join("packs");
    std::fs::create_dir(&packs_dir).unwrap();
    std::fs::rename(dir.path().join("a.tar.gz"), packs_dir.join("a.tar.gz")).unwrap();
    std::fs::rename(dir.path().join("b.tar.gz"), packs_dir.join("b.tar.gz")).unwrap();
    let rows = filter_log(
        home.path(),
        dir.path(),
        &["--source", packs_dir.to_str().unwrap()],
    );
    assert_eq!(rows.len(), 2);
}

#[test]
fn source_same_pack_twice_is_noop_count() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "dup"]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    let pack_arg = pack.to_str().unwrap();
    let once = filter_log(home.path(), dir.path(), &["--source", pack_arg]);
    let twice = filter_log(
        home.path(),
        dir.path(),
        &["--source", pack_arg, "--source", pack_arg],
    );
    assert_eq!(once.len(), twice.len());
}

#[test]
fn source_drops_temp_extract_dirs() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    let scratch = tempdir().unwrap();
    lade_user(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", scratch.path())
        .args([
            "log",
            "--all",
            "--json",
            "--since",
            "1d",
            "--source",
            pack.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(leftover_src_dirs_in(scratch.path()), 0);
}

#[test]
fn usage_source_pack_json() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo \":\n  API_TOKEN: raw://x\n");
    inject(home.path(), dir.path(), &["echo", "hi"]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    let out = lade_user(home.path())
        .current_dir(dir.path())
        .args([
            "usage",
            "--all",
            "--json",
            "--since",
            "1d",
            "--source",
            pack.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["rule"], "^echo ");
}

#[test]
fn source_pack_excludes_later_live_rows() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "packed"]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    inject(home.path(), dir.path(), &["echo", "live-only"]);
    let packed = filter_log(
        home.path(),
        dir.path(),
        &["--source", pack.to_str().unwrap()],
    );
    let cmds: Vec<String> = packed.iter().map(row_line).collect();
    assert!(cmds.iter().any(|c| c == "echo packed"), "{cmds:?}");
    assert!(!cmds.iter().any(|c| c == "echo live-only"), "{cmds:?}");
}

#[test]
fn source_since_filters_packed_window() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "old"]);
    backdate_all(home.path(), "2020-01-01T00:00:00.000Z");
    let pack = share_pack(home.path(), dir.path(), &["--limit", "10"]);
    let recent = filter_log(
        home.path(),
        dir.path(),
        &["--source", pack.to_str().unwrap()],
    );
    assert!(recent.is_empty(), "{recent:?}");
    let out = lade_user(home.path())
        .current_dir(dir.path())
        .args([
            "log",
            "--all",
            "--json",
            "--limit",
            "10",
            "--source",
            pack.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let all: Vec<serde_json::Value> = serde_json::from_slice(&out).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(row_line(&all[0]), "echo old");
}

#[test]
fn source_invalid_pack_and_missing_db_clean_temps() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["true"]);
    let scratch = tempdir().unwrap();
    let junk = dir.path().join("junk.tar.gz");
    fs::write(&junk, b"not a gzip").unwrap();
    lade_user(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", scratch.path())
        .args([
            "log",
            "--all",
            "--json",
            "--since",
            "1d",
            "--source",
            junk.to_str().unwrap(),
        ])
        .assert()
        .code(1);
    assert_eq!(leftover_src_dirs_in(scratch.path()), 0);

    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    fs::write(empty.join("manifest.json"), "{}").unwrap();
    let missing = dir.path().join("missing.tar.gz");
    let status = std::process::Command::new("tar")
        .args([
            "-czf",
            missing.to_str().unwrap(),
            "-C",
            empty.to_str().unwrap(),
            "manifest.json",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    lade_user(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", scratch.path())
        .args([
            "log",
            "--all",
            "--json",
            "--since",
            "1d",
            "--source",
            missing.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("missing events.db"));
    assert_eq!(leftover_src_dirs_in(scratch.path()), 0);
}

#[test]
fn source_directory_is_not_recursive() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "top"]);
    let top = dir.path().join("top.tar.gz");
    share_pack(home.path(), dir.path(), &["-o", top.to_str().unwrap()]);
    inject(home.path(), dir.path(), &["echo", "nested"]);
    let nested = dir.path().join("nested.tar.gz");
    share_pack(home.path(), dir.path(), &["-o", nested.to_str().unwrap()]);
    let packs = dir.path().join("packs");
    let inner = packs.join("deep");
    fs::create_dir_all(&inner).unwrap();
    fs::rename(&top, packs.join("top.tar.gz")).unwrap();
    fs::rename(&nested, inner.join("nested.tar.gz")).unwrap();
    let rows = filter_log(
        home.path(),
        dir.path(),
        &["--source", packs.to_str().unwrap()],
    );
    let cmds: Vec<String> = rows.iter().map(row_line).collect();
    assert_eq!(cmds, vec!["echo top".to_string()]);
}

#[test]
fn log_and_usage_warn_and_still_print_a_tampered_row() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "first"]);
    inject(home.path(), dir.path(), &["echo", "second"]);
    let conn = rusqlite::Connection::open(home.path().join("events.db")).unwrap();
    conn.execute("UPDATE events SET command = 'rm' WHERE seq = 1", [])
        .unwrap();
    let log = lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--all", "--json", "--since", "1d"])
        .assert()
        .success();
    let log_err = String::from_utf8_lossy(&log.get_output().stderr);
    let log_out = String::from_utf8_lossy(&log.get_output().stdout);
    assert!(log_err.contains("tampered"), "{log_err}");
    assert!(log_err.contains("seq=1"), "{log_err}");
    assert!(log_err.contains("0 rows before it"), "{log_err}");
    assert!(log_err.contains("1 row after it"), "{log_err}");
    assert!(
        log_out.contains("rm") || log_out.contains("echo"),
        "{log_out}"
    );
    let usage = lade(home.path())
        .current_dir(dir.path())
        .args(["usage", "--all", "--json", "--since", "1d"])
        .assert()
        .success();
    let usage_err = String::from_utf8_lossy(&usage.get_output().stderr);
    assert!(usage_err.contains("tampered"), "{usage_err}");
    assert!(!usage.get_output().stdout.is_empty());
}

#[test]
fn log_warns_later_tamper_with_rows_before() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "first"]);
    inject(home.path(), dir.path(), &["echo", "second"]);
    let conn = rusqlite::Connection::open(home.path().join("events.db")).unwrap();
    conn.execute("UPDATE events SET command = 'rm' WHERE seq = 2", [])
        .unwrap();
    let log = lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--all", "--json", "--since", "1d"])
        .assert()
        .success();
    let err = String::from_utf8_lossy(&log.get_output().stderr);
    assert!(err.contains("seq=2"), "{err}");
    assert!(err.contains("1 row before it"), "{err}");
    assert!(err.contains("0 rows after it"), "{err}");
}

#[test]
fn intact_log_has_no_tamper_warning() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "ok"]);
    let log = lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--all", "--json", "--since", "1d"])
        .assert()
        .success();
    let err = String::from_utf8_lossy(&log.get_output().stderr);
    assert!(!err.contains("tampered"), "{err}");
}

#[test]
fn log_verify_still_fails_when_tampered() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "ok"]);
    let conn = rusqlite::Connection::open(home.path().join("events.db")).unwrap();
    conn.execute("UPDATE events SET command = 'rm'", [])
        .unwrap();
    lade(home.path())
        .current_dir(dir.path())
        .args(["log", "verify"])
        .assert()
        .failure();
}

#[test]
fn source_pack_warns_when_snapshot_is_tampered() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "packed"]);
    let pack = share_pack(home.path(), dir.path(), &[]);
    let work = tempdir().unwrap();
    let status = std::process::Command::new("tar")
        .args([
            "-xzf",
            pack.to_str().unwrap(),
            "-C",
            work.path().to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let db = work.path().join("events.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("UPDATE events SET command = 'rm'", [])
        .unwrap();
    let broken = dir.path().join("broken.tar.gz");
    let status = std::process::Command::new("tar")
        .current_dir(work.path())
        .args([
            "-czf",
            broken.to_str().unwrap(),
            "manifest.json",
            "events.db",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let log = lade(home.path())
        .current_dir(dir.path())
        .args([
            "log",
            "--all",
            "--json",
            "--since",
            "1d",
            "--source",
            broken.to_str().unwrap(),
        ])
        .assert()
        .success();
    let err = String::from_utf8_lossy(&log.get_output().stderr);
    let out = String::from_utf8_lossy(&log.get_output().stdout);
    assert!(err.contains("tampered"), "{err}");
    assert!(!out.trim().is_empty(), "{out}");
}

#[test]
fn source_local_never_writes_the_live_diary() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["echo", "kept"]);
    let db = home.path().join("events.db");
    let snapshot = || {
        let bytes = fs::read(&db).unwrap();
        let wal = fs::read(home.path().join("events.db-wal")).unwrap_or_default();
        (bytes, wal)
    };
    let before = snapshot();
    lade(home.path())
        .current_dir(dir.path())
        .args([
            "log", "--all", "--json", "--since", "1d", "--source", "local",
        ])
        .assert()
        .success();
    lade(home.path())
        .current_dir(dir.path())
        .args(["log", "verify", "--source", "local"])
        .assert()
        .success();
    assert!(
        before == snapshot(),
        "a --source read changed the live diary"
    );
}

#[test]
fn verify_reports_a_pack_without_a_chain() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    let work = tempdir().unwrap();
    let conn = rusqlite::Connection::open(work.path().join("events.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE events (
           id TEXT PRIMARY KEY, ts TEXT NOT NULL, kind TEXT NOT NULL, via TEXT,
           audience TEXT, actor TEXT, repo TEXT, git_commit TEXT,
           command TEXT NOT NULL, command_truncated INTEGER NOT NULL,
           hydrate_ms REAL, matches BLOB NOT NULL
         );
         INSERT INTO events (id, ts, kind, command, command_truncated, matches)
         VALUES ('1', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'seen', 'echo', 0, jsonb('[]'));",
    )
    .unwrap();
    drop(conn);
    fs::write(work.path().join("manifest.json"), "{}").unwrap();
    let pack = dir.path().join("old.tar.gz");
    let status = std::process::Command::new("tar")
        .current_dir(work.path())
        .args(["-czf", pack.to_str().unwrap(), "manifest.json", "events.db"])
        .status()
        .unwrap();
    assert!(status.success());
    let verify = lade(home.path())
        .current_dir(dir.path())
        .args(["log", "verify", "--source", pack.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&verify.stderr);
    assert_eq!(verify.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("no chain"), "{stderr}");
    let log = lade(home.path())
        .current_dir(dir.path())
        .args([
            "log",
            "--all",
            "--json",
            "--since",
            "1d",
            "--source",
            pack.to_str().unwrap(),
        ])
        .assert()
        .success();
    let err = String::from_utf8_lossy(&log.get_output().stderr);
    let out = String::from_utf8_lossy(&log.get_output().stdout);
    assert!(err.contains("no chain"), "{err}");
    assert!(out.contains("echo"), "{out}");
}

#[test]
fn source_pack_matches_this_root_by_name() {
    let home = tempdir().unwrap();
    let roots = tempdir().unwrap();
    let a = roots.path().join("one/a");
    let other_a = roots.path().join("two/a");
    let b = roots.path().join("three/b");
    for dir in [&a, &other_a, &b] {
        fs::create_dir_all(dir).unwrap();
        common::init_git(dir);
        write_yml(dir, "{}\n");
    }
    inject(home.path(), &a, &["echo", "in-a"]);
    let pack = share_pack(home.path(), &a, &[]);
    let read = |dir: &std::path::Path| {
        let out = lade(home.path())
            .current_dir(dir)
            .args([
                "log",
                "--json",
                "--since",
                "1d",
                "--source",
                pack.to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice::<Vec<serde_json::Value>>(&out).unwrap()
    };
    assert_eq!(read(&other_a).len(), 1);
    assert_eq!(read(&b).len(), 0);
}
