use super::common;
use super::support::*;
use std::fs;
use tempfile::tempdir;

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
