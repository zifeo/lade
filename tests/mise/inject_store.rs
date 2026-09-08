use super::common;
use super::support::*;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn inject_prefers_locked_bin_over_homebrew() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let brew = dir.path().join("brew");
    let installs = dir.path().join("installs");
    fs::create_dir_all(&brew).unwrap();
    fs::create_dir_all(installs.join("jq/1.7.1")).unwrap();
    write_exec(&brew.join("jq"), "echo BREW");
    write_exec(&installs.join("jq/1.7.1/jq"), "echo PINNED");
    write_cached_env(
        home.path(),
        "aqua:jqlang/jq",
        "aqua-jqlang-jq",
        "1.7.1",
        "{}",
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", prepend_path(&brew))
        .args(["inject", "--", "jq"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PINNED"))
        .stdout(predicates::str::contains("BREW").not());
}

#[cfg(unix)]
#[test]
fn inject_does_not_write_mise_files_in_repo() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    fs::create_dir_all(installs.join("jq/1.7.1")).unwrap();
    write_exec(&installs.join("jq/1.7.1/jq"), "echo PINNED");
    write_cached_env(
        home.path(),
        "aqua:jqlang/jq",
        "aqua-jqlang-jq",
        "1.7.1",
        "{}",
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .args(["inject", "--", "jq"])
        .assert()
        .success();
    assert!(!dir.path().join("mise.toml").exists());
    assert!(!dir.path().join("mise.lock").exists());
    assert!(!dir.path().join("mise.local.toml").exists());
}

#[cfg(unix)]
#[test]
fn inject_leaves_existing_mise_toml_untouched() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    fs::create_dir_all(installs.join("jq/1.7.1")).unwrap();
    write_exec(&installs.join("jq/1.7.1/jq"), "echo PINNED");
    write_cached_env(
        home.path(),
        "aqua:jqlang/jq",
        "aqua-jqlang-jq",
        "1.7.1",
        "{}",
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let original = "[tools]\nnode = \"24.16.0\"\n";
    fs::write(dir.path().join("mise.toml"), original).unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .args(["inject", "--", "jq"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        original
    );
}

#[cfg(unix)]
#[test]
fn inject_refuses_version_conflict() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    fs::write(dir.path().join("mise.toml"), "[tools]\njq = \"1.6.0\"\n").unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--", "jq"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("different versions"));
}

#[cfg(unix)]
#[test]
fn inject_refuses_bare_version() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(dir.path().join("lade.yml"), "^jq:\n  jq: \"1.7.1\"\n").unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--", "jq"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("is not a pin"));
}
