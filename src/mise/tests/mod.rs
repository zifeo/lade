use super::*;
pub(crate) use crate::config::Audience;
use crate::config::LadeFile;
use crate::mise::spec::parse;
use tempfile::tempdir;

pub(super) fn write_cached_env(home: &std::path::Path, uri: &str, json: &str) {
    temp_env::with_var("HOME", Some(home), || {
        let spec = parse(uri).unwrap();
        let map: std::collections::HashMap<String, String> = serde_json::from_str(json).unwrap();
        env::store(&spec, &map).unwrap();
    });
}

#[cfg(unix)]
pub(super) fn write_exec(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

pub(super) fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fut)
}

pub(super) fn git_init(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join(".git")).unwrap();
}

pub(super) fn write_foreign_home_mise(home: &std::path::Path) {
    std::fs::create_dir_all(home.join(".config/mise")).unwrap();
    std::fs::write(
        home.join(".config/mise/config.toml"),
        "[tools]\njava = \"21\"\n[env]\nNODE_VERSION = \"24\"\n",
    )
    .unwrap();
    std::fs::write(home.join("mise.toml"), "[tools]\njava = \"21\"\n").unwrap();
}

#[cfg(unix)]
pub(super) fn isolation_record_stub() -> &'static str {
    r#"
cd_dir="."
if [ "$1" = "--cd" ]; then
  cd_dir="$2"
  shift 2
fi
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
if [ "$1" = "latest" ]; then
  printf '%s\n' "1.7.1"
  exit 0
fi
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if [ -n "$MISE_GLOBAL_CONFIG_FILE" ]; then
  while IFS= read -r line; do
    printf '%s\n' "$line"
  done < "$MISE_GLOBAL_CONFIG_FILE" > "$MISE_INSTALLS_DIR/isolated.toml"
fi
printf '%s\n' "$MISE_IGNORED_CONFIG_PATHS" > "$MISE_INSTALLS_DIR/ignored"
printf '%s\n' "$MISE_CONFIG_DIR" > "$MISE_INSTALLS_DIR/config-dir"
printf '%s\n' "$MISE_SHIMS_DIR" > "$MISE_INSTALLS_DIR/shims-dir"
if [ "$1" = "lock" ]; then
  toml="$cd_dir/mise.toml"
  lock="$cd_dir/mise.lock"
  if [ -f "$toml" ]; then
    {
      printf '%s\n' '# @generated'
      printf '%s\n' ''
      printf '%s\n' 'lockfile_version = 2'
      printf '%s\n' ''
      awk '
        BEGIN { in_tools=0 }
        /^\[tools\]/ { in_tools=1; next }
        /^\[/ { in_tools=0 }
        in_tools && /=/ {
          line=$0
          sub(/^[[:space:]]+/, "", line)
          split(line, parts, "=")
          key=parts[1]
          ver=parts[2]
          sub(/^[[:space:]]+/, "", key)
          sub(/[[:space:]]+$/, "", key)
          sub(/^[[:space:]]+/, "", ver)
          sub(/[[:space:]]+$/, "", ver)
          gsub(/^"/, "", key)
          gsub(/"$/, "", key)
          gsub(/^"/, "", ver)
          gsub(/"$/, "", ver)
          printf "[[tools.\"%s\"]]\n", key
          printf "version = \"%s\"\n", ver
          printf "backend = \"%s\"\n", key
          printf "checksum = \"sha256:deadbeef\"\n"
          printf "\n"
          printf "[tools.\"%s\".\"platforms.macos-arm64\"]\n", key
          printf "url = \"https://example.com/%s\"\n", ver
          printf "\n"
        }
      ' "$toml"
    } > "$lock"
  fi
  exit 0
fi
if printf '%s' "$*" | grep -q -- '--json-extended'; then
  printf '%s\n' '{}'
  exit 0
fi
mkdir -p "$MISE_INSTALLS_DIR/jq/1.7.1"
printf '#!/bin/sh\necho PINNED\n' > "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
chmod 755 "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
exit 0
"#
}

pub(super) fn assert_isolated_pin_only(
    installs: &std::path::Path,
    home: &std::path::Path,
    version: &str,
) {
    let isolated = std::fs::read_to_string(installs.join("isolated.toml")).unwrap();
    assert!(isolated.contains("[tools]"), "{isolated}");
    assert!(isolated.contains(version), "{isolated}");
    assert!(!isolated.contains("java"), "{isolated}");
    assert!(!isolated.contains("NODE_VERSION"), "{isolated}");
    let ignored = std::fs::read_to_string(installs.join("ignored")).unwrap();
    assert!(
        ignored.contains(&home.join(".config/mise").display().to_string()),
        "{ignored}"
    );
    assert!(
        ignored.contains(&home.join("mise.toml").display().to_string()),
        "{ignored}"
    );
    let shims = std::fs::read_to_string(installs.join("shims-dir")).unwrap();
    assert!(shims.contains("mise-shims"), "{shims}");
    assert!(
        !shims.contains(&home.join(".local/share/mise/shims").display().to_string()),
        "{shims}"
    );
}

mod ensure_flow;
mod install_flow;
mod lock;
mod lock_parse;
mod prepare;
mod prepare_implied;
mod prepare_refresh;
mod repo_lock;
mod setup;
mod setup_toml;
