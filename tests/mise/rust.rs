use super::common;
use super::support::*;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn inject_stale_sidecar_does_not_export_java_home() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = dir.path().join("bin");
    fs::create_dir_all(installs.join("rust/1.96.0")).unwrap();
    fs::create_dir_all(&bin).unwrap();
    write_exec(
        &installs.join("rust/1.96.0/cargo"),
        r#"printf '%s\n' "JAVA=${JAVA_HOME-}""#,
    );
    let sidecar_dir = temp_env::with_var("HOME", Some(home.path()), || {
        directories::ProjectDirs::from("com", "zifeo", "lade")
            .expect("project dirs")
            .cache_dir()
            .join("mise-env/core-rust")
    });
    fs::create_dir_all(&sidecar_dir).unwrap();
    fs::write(
        sidecar_dir.join("1.96.0.json"),
        r#"{"JAVA_HOME":"/opt/java","RUSTUP_TOOLCHAIN":"1.96.0"}"#,
    )
    .unwrap();
    write_exec(
        &bin.join("mise"),
        r#"printf '%s\n' '{"RUSTUP_TOOLCHAIN":{"value":"1.96.0","tool":"core:rust"},"JAVA_HOME":{"value":"/opt/java","tool":"java"}}'"#,
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", prepend_path(&bin))
        .env("JAVA_HOME", "/parent-java")
        .args(["inject", "--", "cargo", "test"])
        .assert()
        .success()
        .stdout(predicates::str::contains("JAVA=/parent-java"))
        .stdout(predicates::str::contains("/opt/java").not());
}

#[cfg(unix)]
#[test]
fn inject_rust_pin_exports_toolchain() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    fs::create_dir_all(installs.join("rust/1.96.0")).unwrap();
    write_exec(
        &installs.join("rust/1.96.0/cargo"),
        r#"printf '%s\n' "$RUSTUP_TOOLCHAIN""#,
    );
    write_cached_env(
        home.path(),
        "core:rust",
        "core-rust",
        "1.96.0",
        r#"{"RUSTUP_TOOLCHAIN":"1.96.0"}"#,
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env_remove("RUSTUP_TOOLCHAIN")
        .args(["inject", "--", "cargo", "test"])
        .assert()
        .success()
        .stdout(predicates::str::contains("1.96.0"));
}

#[cfg(unix)]
#[test]
fn inject_rustc_and_cargo_share_pin() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let rust = installs.join("rust/1.96.0");
    fs::create_dir_all(&rust).unwrap();
    write_exec(
        &rust.join("cargo"),
        r#"printf '%s\n' "CARGO=$RUSTUP_TOOLCHAIN""#,
    );
    write_exec(
        &rust.join("rustc"),
        r#"printf '%s\n' "RUSTC=$RUSTUP_TOOLCHAIN""#,
    );
    write_cached_env(
        home.path(),
        "core:rust",
        "core-rust",
        "1.96.0",
        r#"{"RUSTUP_TOOLCHAIN":"1.96.0"}"#,
    );
    fs::write(
        dir.path().join("lade.yml"),
        ".:\n  cargo: mise://core/rust@1.96.0\n  rustc: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.80.0\"\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env_remove("RUSTUP_TOOLCHAIN")
        .args(["inject", "--", "cargo", "test"])
        .assert()
        .success()
        .stdout(predicates::str::contains("CARGO=1.96.0"));
    common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env_remove("RUSTUP_TOOLCHAIN")
        .args(["inject", "--", "rustc", "--version"])
        .assert()
        .success()
        .stdout(predicates::str::contains("RUSTC=1.96.0"));
}

#[cfg(unix)]
#[test]
fn inject_finds_mise_bins_layout() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bins = installs.join("jq/1.7.1/.mise-bins");
    fs::create_dir_all(&bins).unwrap();
    write_exec(&bins.join("jq"), "echo PINNED");
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
        .success()
        .stdout(predicates::str::contains("PINNED"));
}
