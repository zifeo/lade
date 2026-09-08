use super::*;
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
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if [ -n "$MISE_GLOBAL_CONFIG_FILE" ]; then
  while IFS= read -r line; do
    printf '%s\n' "$line"
  done < "$MISE_GLOBAL_CONFIG_FILE" > "$MISE_INSTALLS_DIR/isolated.toml"
fi
printf '%s\n' "$MISE_IGNORED_CONFIG_PATHS" > "$MISE_INSTALLS_DIR/ignored"
printf '%s\n' "$MISE_CONFIG_DIR" > "$MISE_INSTALLS_DIR/config-dir"
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
}

mod install_flow;
mod lock;
mod prepare;
