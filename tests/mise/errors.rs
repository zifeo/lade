use super::common;
use super::support::*;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn inject_refuses_homebrew_when_install_leaves_bin_missing() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let brew = dir.path().join("brew");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&installs).unwrap();
    fs::create_dir_all(&brew).unwrap();
    fs::create_dir_all(&bin).unwrap();
    write_exec(&brew.join("jq"), "echo BREW");
    write_exec(&bin.join("mise"), "exit 0");
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", format!("{}:{}", brew.display(), prepend_path(&bin)))
        .args(["inject", "--", "jq"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("Could not run the locked"))
        .stderr(predicates::str::contains("Homebrew"))
        .stdout(predicates::str::contains("BREW").not());
}

#[cfg(unix)]
#[test]
fn inject_install_failure_bubbles_mise_stderr() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&installs).unwrap();
    fs::create_dir_all(&bin).unwrap();
    write_exec(
        &bin.join("mise"),
        "printf '%s\\n' UNIQUE_MISE_FAIL >&2; exit 1",
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", prepend_path(&bin))
        .args(["inject", "--", "jq"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("mise install failed"))
        .stderr(predicates::str::contains("UNIQUE_MISE_FAIL"));
}

#[cfg(unix)]
#[test]
fn inject_missing_mise_is_an_error() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    fs::create_dir_all(&installs).unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let empty = dir.path().join("empty-path");
    fs::create_dir_all(&empty).unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", &empty)
        .args(["inject", "--", "jq"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("Could not run mise"));
}

#[cfg(unix)]
#[test]
fn inject_legacy_cli_is_an_error() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: core:rust@1.96.0\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "--", "cargo", "test"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("mise://core/rust@1.96.0"));
}

#[cfg(unix)]
#[test]
fn inject_lock_mismatch_skips_locked_flag() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&installs).unwrap();
    fs::create_dir_all(&bin).unwrap();
    write_exec(
        &bin.join("mise"),
        r#"
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if printf '%s' "$*" | grep -q -- '--json-extended'; then
  printf '%s\n' '{}'
  exit 0
fi
mkdir -p "$MISE_INSTALLS_DIR/jq/1.7.1"
printf '#!/bin/sh\necho PINNED\n' > "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
chmod 755 "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
exit 0
"#,
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.6.0\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", prepend_path(&bin))
        .args(["inject", "--", "jq"])
        .assert()
        .success();
    let args = fs::read_to_string(installs.join("mise-args")).unwrap();
    assert!(args.contains("install"), "{args}");
    assert!(!args.contains("--locked"), "{args}");
}

#[cfg(unix)]
#[test]
fn inject_lock_backend_id_key_uses_locked() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&installs).unwrap();
    fs::create_dir_all(&bin).unwrap();
    write_exec(
        &bin.join("mise"),
        r#"
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if printf '%s' "$*" | grep -q -- '--json-extended'; then
  printf '%s\n' '{}'
  exit 0
fi
mkdir -p "$MISE_INSTALLS_DIR/jq/1.7.1"
printf '#!/bin/sh\necho PINNED\n' > "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
chmod 755 "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
exit 0
"#,
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("mise.lock"),
        "[[tools.\"aqua:jqlang/jq\"]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", prepend_path(&bin))
        .args(["inject", "--", "jq"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PINNED"));
    let args = fs::read_to_string(installs.join("mise-args")).unwrap();
    assert!(args.contains("--locked"), "{args}");
}

#[cfg(unix)]
#[test]
fn inject_catch_all_pins_cargo_not_echo() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    fs::create_dir_all(installs.join("rust/1.96.0")).unwrap();
    write_exec(&installs.join("rust/1.96.0/cargo"), "echo PINNED");
    write_cached_env(
        home.path(),
        "core:rust",
        "core-rust",
        "1.96.0",
        r#"{"RUSTUP_TOOLCHAIN":"1.96.0"}"#,
    );
    fs::write(
        dir.path().join("lade.yml"),
        ".:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .args(["inject", "--", "cargo", "test"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PINNED"));
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .args(["inject", "--", "echo", "hi"])
        .assert()
        .success()
        .stdout(predicates::str::contains("hi"));
}
