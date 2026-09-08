mod common;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[cfg(unix)]
fn write_cached_env(home: &Path, tool: &str, slug: &str, version: &str, env_json: &str) {
    let dir = temp_env::with_var("HOME", Some(home), || {
        directories::ProjectDirs::from("com", "zifeo", "lade")
            .expect("project dirs")
            .cache_dir()
            .join("mise-env")
            .join(slug)
    });
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("{version}.json")),
        format!(r#"{{"v":1,"tool":"{tool}","env":{env_json}}}"#),
    )
    .unwrap();
}

#[cfg(unix)]
fn write_exec(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn prepend_path(dir: &Path) -> String {
    let old = std::env::var("PATH").unwrap_or_default();
    format!("{}:{old}", dir.display())
}

fn export_value(stdout: &str, key: &str) -> Option<String> {
    let needle = format!("export {key}='");
    let rest = stdout.split(&needle).nth(1)?;
    Some(rest.split('\'').next()?.to_string())
}

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

#[cfg(unix)]
#[test]
fn inject_installs_on_need_then_runs_store_bin() {
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
if [ -n "$MISE_GLOBAL_CONFIG_FILE" ]; then
  if printf '%s' "$*" | grep -q -- '--json-extended'; then
    dest="$MISE_INSTALLS_DIR/isolated-env.toml"
  else
    dest="$MISE_INSTALLS_DIR/isolated-install.toml"
  fi
  while IFS= read -r line; do
    printf '%s\n' "$line"
  done < "$MISE_GLOBAL_CONFIG_FILE" > "$dest"
fi
printf '%s\n' "$MISE_IGNORED_CONFIG_PATHS" > "$MISE_INSTALLS_DIR/ignored"
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
    fs::create_dir_all(home.path().join(".config/mise")).unwrap();
    fs::write(
        home.path().join(".config/mise/config.toml"),
        "[tools]\njava = \"21\"\n[env]\nNODE_VERSION = \"24\"\n",
    )
    .unwrap();
    fs::write(home.path().join("mise.toml"), "[tools]\njava = \"21\"\n").unwrap();
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
        .env("PATH", prepend_path(&bin))
        .args(["inject", "--", "jq"])
        .assert()
        .success()
        .stdout(predicates::str::contains("PINNED"));
    let args = fs::read_to_string(installs.join("mise-args")).unwrap();
    assert!(args.contains("install"), "{args}");
    assert!(args.contains("--locked"), "{args}");
    assert!(args.contains("env"), "{args}");
    assert!(args.contains("--json-extended"), "{args}");
    assert!(!dir.path().join("mise.toml").exists());
    for name in ["isolated-install.toml", "isolated-env.toml"] {
        let isolated = fs::read_to_string(installs.join(name)).unwrap();
        assert!(isolated.contains("1.7.1"), "{name} {isolated}");
        assert!(!isolated.contains("java"), "{name} {isolated}");
        assert!(!isolated.contains("NODE_VERSION"), "{name} {isolated}");
    }
    let ignored = fs::read_to_string(installs.join("ignored")).unwrap();
    assert!(
        ignored.contains(&home.path().join(".config/mise").display().to_string()),
        "{ignored}"
    );
}

#[cfg(unix)]
#[test]
fn inject_installs_from_url_isolates_pin_only_config() {
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
if [ -n "$MISE_GLOBAL_CONFIG_FILE" ]; then
  if printf '%s' "$*" | grep -q -- '--json-extended'; then
    dest="$MISE_INSTALLS_DIR/isolated-env.toml"
  else
    dest="$MISE_INSTALLS_DIR/isolated-install.toml"
  fi
  while IFS= read -r line; do
    printf '%s\n' "$line"
  done < "$MISE_GLOBAL_CONFIG_FILE" > "$dest"
fi
printf '%s\n' "$MISE_IGNORED_CONFIG_PATHS" > "$MISE_INSTALLS_DIR/ignored"
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
    fs::create_dir_all(home.path().join(".config/mise")).unwrap();
    fs::write(
        home.path().join(".config/mise/config.toml"),
        "[tools]\njava = \"21\"\n",
    )
    .unwrap();
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
        .success()
        .stdout(predicates::str::contains("PINNED"));
    let args = fs::read_to_string(installs.join("mise-args")).unwrap();
    assert!(args.contains("install"), "{args}");
    assert!(!args.contains("--locked"), "{args}");
    let isolated = fs::read_to_string(installs.join("isolated-install.toml")).unwrap();
    assert!(isolated.contains("1.7.1"), "{isolated}");
    assert!(!isolated.contains("java"), "{isolated}");
    let ignored = fs::read_to_string(installs.join("ignored")).unwrap();
    assert!(
        ignored.contains(&home.path().join(".config/mise").display().to_string()),
        "{ignored}"
    );
}

#[cfg(unix)]
#[test]
fn set_mise_ls_ignores_home_java_keeps_project_node() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = home.path().join("tickets");
    fs::create_dir_all(&tickets).unwrap();
    fs::create_dir_all(home.path().join(".config/mise")).unwrap();
    fs::write(
        home.path().join(".config/mise/config.toml"),
        "[tools]\njava = \"21\"\n[env]\nNODE_VERSION = \"24\"\n",
    )
    .unwrap();
    fs::write(home.path().join("mise.toml"), "[tools]\njava = \"21\"\n").unwrap();
    fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nnode = \"24.16.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "^mise:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", &tickets)
        .args(["set", "mise", "ls"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&stdout);
    let composed_path =
        export_value(&stdout, "MISE_GLOBAL_CONFIG_FILE").expect("composed mise config");
    let composed = fs::read_to_string(composed_path).unwrap();
    assert!(composed.contains("node = \"24.16.0\""), "{composed}");
    assert!(composed.contains("aqua:jqlang/jq"), "{composed}");
    assert!(!composed.contains("java"), "{composed}");
    assert!(!composed.contains("NODE_VERSION"), "{composed}");
    assert!(stdout.contains("export MISE_CONFIG_DIR="), "{stdout}");
    let ignored = export_value(&stdout, "MISE_IGNORED_CONFIG_PATHS").expect("ignored paths");
    assert!(
        ignored.contains(&home.path().join(".config/mise").display().to_string()),
        "{ignored}"
    );
}

#[cfg(unix)]
#[test]
fn set_then_unset_leaves_repo_without_mise_files() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = home.path().join("tickets");
    fs::create_dir_all(&tickets).unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "^mise:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let set = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", &tickets)
        .args(["set", "mise", "ls"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&set);
    assert!(
        stdout.contains("MISE_GLOBAL_CONFIG_FILE") || stdout.contains("LADE_MISE_CONFIG"),
        "{stdout}"
    );
    assert!(!dir.path().join("mise.toml").exists());
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", &tickets)
        .env(
            "LADE_MISE_CONFIG",
            tickets
                .read_dir()
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("lade-mise-") && n.ends_with(".toml"))
                })
                .expect("temp mise file"),
        )
        .args(["unset", "mise", "ls"])
        .assert()
        .success();
    let leftovers: Vec<_> = tickets
        .read_dir()
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("lade-mise-"))
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert!(!dir.path().join("mise.toml").exists());
    assert!(!dir.path().join("mise.lock").exists());
}

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

#[cfg(unix)]
#[test]
fn set_jq_pin_exports_store_path_first() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("jq/1.7.1");
    fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("jq"), "echo PINNED");
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
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", "/usr/bin")
        .args(["set", "jq", "."])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&stdout);
    let path = export_value(&stdout, "PATH").expect("PATH");
    assert!(
        path.starts_with(&format!("{}/", bin.display()))
            || path.starts_with(&format!("{}:", bin.display())),
        "{path}"
    );
}

#[cfg(unix)]
#[test]
fn unset_restores_path_after_jq_pin_set() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("jq/1.7.1");
    fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("jq"), "echo PINNED");
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
    let set_stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .env("MISE_INSTALLS_DIR", &installs)
            .env("PATH", "/usr/bin")
            .args(["set", "jq", "."])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .into_owned();
    let restore = export_value(&set_stdout, "LADE_RESTORE").expect("LADE_RESTORE");
    let unset_stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .env("MISE_INSTALLS_DIR", &installs)
            .env("LADE_RESTORE", restore)
            .args(["unset", "jq", "."])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .into_owned();
    assert!(
        unset_stdout.contains("export PATH='/usr/bin'"),
        "{unset_stdout}"
    );
}

#[cfg(unix)]
#[test]
fn set_refuses_mise_toml_version_conflict() {
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
        .args(["set", "jq"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("different versions"));
}

#[cfg(unix)]
#[test]
fn inject_mise_intercept_removes_temp_config_after_run() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = home.path().join("tickets");
    fs::create_dir_all(&tickets).unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "^mise:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", &tickets)
        .args(["inject", "--", "mise", "ls"])
        .assert()
        .success();
    let leftovers: Vec<_> = tickets
        .read_dir()
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("lade-mise-"))
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[cfg(unix)]
#[test]
fn hook_inject_runs_pinned_cargo() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tmp = tempdir().unwrap();
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
        "^cargo:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    let payload = r#"{"tool_name":"Shell","tool_input":{"command":"cargo test"},"hook_event_name":"preToolUse","conversation_id":"conv_1"}"#;
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .env("CURSOR_VERSION", "1.0")
        .env("MISE_INSTALLS_DIR", &installs)
        .args(["hook", "--harness", "cursor"])
        .write_stdin(payload)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let command = parsed["updated_input"]["command"]
        .as_str()
        .or_else(|| parsed["hookSpecificOutput"]["updatedInput"]["command"].as_str())
        .expect("wrap command");
    assert!(command.contains("--pretool="), "{command}");
    let id_start = command.find("--pretool=").expect("pretool") + "--pretool=".len();
    let id: String = command[id_start..].chars().take(4).collect();
    common::lade(home.path())
        .current_dir(dir.path())
        .env("TMPDIR", tmp.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .args([format!("--pretool={id}"), "cargo".into(), "test".into()])
        .assert()
        .success()
        .stdout(predicates::str::contains("PINNED"));
}
