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
        .stderr(predicates::str::contains("Could not run mise"))
        .stderr(predicates::str::contains("lade setup"))
        .stderr(predicates::str::contains(
            "https://mise.jdx.dev/installing-mise.html",
        ));
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
        r#"# @generated
lockfile_version = 2

[[tools."aqua:jqlang/jq"]]
version = "1.7.1"
backend = "aqua:jqlang/jq"

[tools."aqua:jqlang/jq"."platforms.macos-arm64"]
url = "https://example.com/jq"
"#,
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
fn inject_catch_all_pin_is_on_which_path() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("kubectl/1.37.0");
    fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("kubectl"), "echo PINNED");
    write_cached_env(
        home.path(),
        "aqua:kubernetes/kubectl",
        "aqua-kubernetes-kubectl",
        "1.37.0",
        "{}",
    );
    fs::write(
        dir.path().join("lade.yml"),
        ".:\n  kubectl: mise://aqua/kubernetes/kubectl@1.37.0\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .args(["inject", "--", "kubectl", "version"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PINNED"));
    let which = common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .args(["inject", "--", "which", "kubectl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let which = String::from_utf8_lossy(&which);
    assert!(
        which.contains(&bin.join("kubectl").display().to_string()),
        "{which}"
    );
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .args(["inject", "--", "echo", "hi"])
        .assert()
        .success()
        .stdout(predicates::str::contains("hi"));
}

#[cfg(unix)]
#[test]
fn inject_command_pin_does_not_apply_to_which() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("kubectl/1.37.0");
    let brew = dir.path().join("brew");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&brew).unwrap();
    write_exec(&bin.join("kubectl"), "echo PINNED");
    write_exec(&brew.join("kubectl"), "echo BREW");
    write_cached_env(
        home.path(),
        "aqua:kubernetes/kubectl",
        "aqua-kubernetes-kubectl",
        "1.37.0",
        "{}",
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^kubectl:\n  kubectl: mise://aqua/kubernetes/kubectl@1.37.0\n",
    )
    .unwrap();
    let path = format!("{}:{}", brew.display(), prepend_path(&bin));
    let which = common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", &path)
        .args(["inject", "--", "which", "kubectl"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let which = String::from_utf8_lossy(&which);
    assert!(
        which.contains(&brew.join("kubectl").display().to_string()),
        "{which}"
    );
    assert!(
        !which.contains(&bin.join("kubectl").display().to_string()),
        "{which}"
    );
}
