#![cfg(unix)]
//! A pre-tool line that starts with `cd <dir> &&` reads that directory's
//! rules in the wrap too, and caches under them: a value from `sub` never
//! lands in the body of a rule from its parent.

mod common;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

const LINE: &str = "cd sub && printenv TOKEN";

fn secret(dir: &Path, name: &str, value: &str) -> String {
    let path = dir.join(format!("{name}.json"));
    fs::write(&path, format!(r#"{{"v":"{value}"}}"#)).unwrap();
    format!("file://{}?query=.v", path.display())
}

fn wrap(home: &Path, root: &Path, line: &str) -> String {
    let mut cmd = common::lade(home);
    cmd.current_dir(root)
        .args(["--pretool", "inject", "--no-mask", line]);
    let out = cmd.assert().success().get_output().stdout.clone();
    String::from_utf8(out).unwrap()
}

/// `root` injects `TOKEN` into every command, `root/sub` overrides it for
/// `printenv`.
fn repo(root: &Path, dev: &str, prod: &str) {
    let sub = root.join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(
        root.join("lade.yml"),
        format!("\".\":\n  TOKEN: \"{dev}\"\n"),
    )
    .unwrap();
    fs::write(
        sub.join("lade.yml"),
        format!("\"^printenv\":\n  TOKEN: \"{prod}\"\n"),
    )
    .unwrap();
}

#[test]
fn leading_cd_reads_the_rules_of_that_directory() {
    let (home, root) = (tempdir().unwrap(), tempdir().unwrap());
    repo(root.path(), "dev", "prod");
    assert_eq!(wrap(home.path(), root.path(), LINE), "prod\n");
    assert_eq!(wrap(home.path(), root.path(), "printenv TOKEN"), "dev\n");
}

#[test]
fn a_value_from_a_subdirectory_is_never_cached_for_its_parent() {
    let (home, root, store) = (tempdir().unwrap(), tempdir().unwrap(), tempdir().unwrap());
    let dev = secret(store.path(), "dev", "dev");
    let prod = secret(store.path(), "prod", "prod");
    repo(root.path(), &dev, &prod);
    assert_eq!(wrap(home.path(), root.path(), LINE), "prod\n");
    assert_eq!(wrap(home.path(), root.path(), "printenv TOKEN"), "dev\n");
    // Served from the hub under the rules of `sub`, not hydrated again.
    secret(store.path(), "prod", "rotated");
    assert_eq!(wrap(home.path(), root.path(), LINE), "prod\n");
}

#[test]
fn set_and_inject_follow_a_leading_cd_too() {
    let (home, root) = (tempdir().unwrap(), tempdir().unwrap());
    repo(root.path(), "dev", "prod");
    common::lade(home.path())
        .current_dir(root.path())
        .args(["set", LINE])
        .assert()
        .success()
        .stdout(predicates::str::contains("TOKEN='prod'"));
    common::lade(home.path())
        .current_dir(root.path())
        .args(["inject", "--no-mask", LINE])
        .assert()
        .success()
        .stdout("prod\n");
}

#[test]
fn an_agent_reads_no_progress_and_one_line_per_failure() {
    let (home, root) = (tempdir().unwrap(), tempdir().unwrap());
    repo(root.path(), "dev", "prod");
    let out = common::lade(home.path())
        .current_dir(root.path())
        .env("CLAUDECODE", "1")
        .args(["--pretool", "inject", LINE])
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    fs::write(
        root.path().join("lade.yml"),
        "\".\":\n  TOKEN: \"sh://exit 3\"\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(root.path())
        .env("CLAUDECODE", "1")
        .args(["--pretool", "inject", "printenv TOKEN"])
        .assert()
        .failure()
        .get_output()
        .clone();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.starts_with("lade: error: "), "{err}");
    assert!(err.contains("did not run"), "{err}");
}
