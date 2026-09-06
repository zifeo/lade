mod common;
use common::{
    backdate_all, filter_log, inject, lade_user, leftover_src_dirs_in, row_line, share_pack,
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
