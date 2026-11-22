mod common;
use tempfile::tempdir;

#[test]
fn log_is_not_inject_alias() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--all"])
        .assert()
        .success();
}

#[test]
fn log_help_prints_db_path_and_duration() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let db = home.path().join("events.db");
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains(db.to_string_lossy().as_ref()))
        .stdout(predicates::str::contains("Nmonth"));
}

fn hub_state(home: &std::path::Path, dir: &std::path::Path) -> String {
    let out = common::lade(home)
        .current_dir(dir)
        .args(["ttl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap()
}

#[test]
fn prune_hub_stops_the_hub() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["ttl", "set", "2h"])
        .assert()
        .success();
    let listed = hub_state(home.path(), dir.path());
    assert!(!listed.contains("hub: down"), "{listed}");
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["log", "prune", "--hub"])
        .assert()
        .success();
    let listed = hub_state(home.path(), dir.path());
    assert!(listed.contains("hub: down"), "{listed}");
}

#[test]
fn hub_hit_is_cached_in_diary() {
    for vehicle in [&["--"][..], &["mcp", "--"][..]] {
        let home = tempdir().unwrap();
        let dir = tempdir().unwrap();
        let secret = dir.path().join("secret.json");
        std::fs::write(&secret, r#"{"token":"first-value"}"#).unwrap();
        std::fs::write(
            dir.path().join("lade.yml"),
            format!(
                "\"^true$\":\n  \".\":\n    log: true\n  KEY: \"file://{}?query=.token\"\n",
                secret.display()
            ),
        )
        .unwrap();
        // A wrap gives a cold hub a few ms: start it first.
        for args in [&["ttl", "set", "5m"][..], &["ttl", "unset"][..]] {
            common::lade(home.path())
                .current_dir(dir.path())
                .args(args)
                .assert()
                .success();
        }
        for _ in 0..2 {
            common::lade(home.path())
                .current_dir(dir.path())
                .args(vehicle)
                .arg("true")
                .assert()
                .success();
        }
        common::lade(home.path())
            .current_dir(dir.path())
            .args(["ttl", "forget"])
            .assert()
            .success();
        let rows = common::log_rows(home.path(), dir.path());
        let mut cached: Vec<bool> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row["matches"][0]["bindings"]
                    .as_array()
                    .is_some_and(|bindings| bindings.iter().any(|b| b["cached"] == true))
            })
            .collect();
        cached.sort();
        assert_eq!(cached, [false, true], "{vehicle:?}: {rows}");
    }
}

#[test]
fn queries_write_no_row() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::write_yml(dir.path(), "\"^echo\":\n  A: one\n");
    common::inject(home.path(), dir.path(), &["echo", "hi"]);
    let count = || {
        rusqlite::Connection::open(home.path().join("events.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM events", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    assert_eq!(count(), 1);
    for args in [
        &["log", "--all"][..],
        &["usage", "--all"],
        &["status"],
        &["bench"],
    ] {
        common::lade(home.path())
            .current_dir(dir.path())
            .args(args)
            .assert()
            .success();
        assert_eq!(count(), 1, "{args:?} wrote a row");
    }
}
