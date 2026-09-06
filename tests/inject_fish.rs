mod common;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
fn write_fish_overwrite_fixture(dir: &std::path::Path, home: &std::path::Path) {
    let fish_dir = home.join(".config/fish");
    fs::create_dir_all(&fish_dir).unwrap();
    fs::write(
        fish_dir.join("config.fish"),
        "set -x SECRET from_fish_profile\n",
    )
    .unwrap();
    fs::write(
        dir.join("lade.yml"),
        "\"echo.*\":\n  SECRET: 'sh://printf %s resolved_secret_fish42'\n",
    )
    .unwrap();
}

#[test]
#[cfg(unix)]
fn test_inject_survives_fish_profile_overwrite() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    write_fish_overwrite_fixture(dir.path(), home.path());
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_SHELL", "fish")
        .env_remove("XDG_CONFIG_HOME")
        .args(["inject", "--no-mask", "echo", "$SECRET"])
        .assert()
        .success()
        .stdout(predicates::str::contains("resolved_secret_fish42"))
        .stdout(predicates::str::contains("from_fish_profile").not());
}

#[test]
#[cfg(unix)]
fn test_inject_masks_secret_when_fish_profile_would_overwrite() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    write_fish_overwrite_fixture(dir.path(), home.path());
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_SHELL", "fish")
        .env_remove("XDG_CONFIG_HOME")
        .args(["inject", "echo", "$SECRET"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(
        !stdout.contains("resolved_secret_fish42"),
        "resolved secret leaked into output: {stdout}"
    );
    assert!(
        !stdout.contains("from_fish_profile"),
        "profile value leaked; resolved secret did not reach the child: {stdout}"
    );
    assert!(
        stdout.contains("${SECRET:-REDACTED}"),
        "expected redaction token in output: {stdout}"
    );
}
