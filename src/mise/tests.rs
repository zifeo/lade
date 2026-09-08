use super::*;
use crate::config::LadeFile;
use crate::mise::spec::parse;
use tempfile::tempdir;

fn write_cached_env(home: &std::path::Path, uri: &str, json: &str) {
    temp_env::with_var("HOME", Some(home), || {
        let spec = parse(uri).unwrap();
        let map: std::collections::HashMap<String, String> = serde_json::from_str(json).unwrap();
        env::store(&spec, &map).unwrap();
    });
}

#[cfg(unix)]
fn write_exec(path: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fut)
}

fn write_foreign_home_mise(home: &std::path::Path) {
    std::fs::create_dir_all(home.join(".config/mise")).unwrap();
    std::fs::write(
        home.join(".config/mise/config.toml"),
        "[tools]\njava = \"21\"\n[env]\nNODE_VERSION = \"24\"\n",
    )
    .unwrap();
    std::fs::write(home.join("mise.toml"), "[tools]\njava = \"21\"\n").unwrap();
}

#[cfg(unix)]
fn isolation_record_stub() -> &'static str {
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

fn assert_isolated_pin_only(installs: &std::path::Path, home: &std::path::Path, version: &str) {
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

#[cfg(unix)]
#[test]
fn happy_path_prepends_store_without_mise() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    let bin = installs.join("jq/1.7.1");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&bin.join("jq"), "echo PINNED");
    write_exec(
        &stub.join("mise"),
        r#"printf '%s\n' called > "$MISE_INSTALLS_DIR/mise-ran""#,
    );
    write_cached_env(home.path(), "mise://aqua/jqlang/jq@1.7.1", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "jq .", dir.path(), &None)).unwrap();
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
            assert!(out.cleanup.is_empty());
            assert!(!installs.join("mise-ran").exists());
        },
    );
}

#[test]
fn bare_version_is_an_error() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), "^jq:\n  jq: \"1.7.1\"\n").unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let err = block_on(prepare(&config, "jq", dir.path(), &None)).unwrap_err();
        assert!(err.to_string().contains("is not a pin"), "{err}");
    });
}

#[test]
fn conflict_with_mise_toml_refuses() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("mise.toml"), "[tools]\njq = \"1.6.0\"\n").unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let err = block_on(prepare(&config, "jq", dir.path(), &None)).unwrap_err();
        assert!(err.to_string().contains("different versions"), "{err}");
    });
}

#[cfg(unix)]
#[test]
fn rust_pin_sets_toolchain_without_mise_spawn() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("rust/1.96.0");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("cargo"), "echo PINNED");
    write_cached_env(
        home.path(),
        "mise://core/rust@1.96.0",
        r#"{"RUSTUP_TOOLCHAIN":"1.96.0","CARGO_HOME":"/c"}"#,
    );
    std::fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
            ("PATH", Some(std::path::Path::new("/usr/bin"))),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "cargo test", dir.path(), &None)).unwrap();
            assert_eq!(out.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            assert_eq!(out.env.get("CARGO_HOME").unwrap(), "/c");
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
        },
    );
}

#[cfg(unix)]
#[test]
fn catch_all_rule_pins_by_argv0() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("rust/1.96.0");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("cargo"), "echo PINNED");
    write_cached_env(
        home.path(),
        "mise://core/rust@1.96.0",
        r#"{"RUSTUP_TOOLCHAIN":"1.96.0"}"#,
    );
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
            ("PATH", Some(std::path::Path::new("/usr/bin"))),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let cargo = block_on(prepare(&config, "cargo test", dir.path(), &None)).unwrap();
            assert_eq!(cargo.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            let echo = block_on(prepare(&config, "echo hi", dir.path(), &None)).unwrap();
            assert!(echo.is_empty());
        },
    );
}

#[cfg(unix)]
#[test]
fn missing_sidecar_asks_mise_env_once() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("rust/1.96.0");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&bin.join("cargo"), "echo PINNED");
    write_exec(
        &stub.join("mise"),
        r#"
printf '%s\n' "1" >> "$MISE_INSTALLS_DIR/mise-count"
printf '%s\n' '{"PATH":"/usr/bin","MISE_YES":"1","RUSTUP_TOOLCHAIN":{"value":"1.96.0","tool":"core:rust"}}'
"#,
    );
    std::fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "cargo test", dir.path(), &None)).unwrap();
            assert_eq!(out.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            assert!(!out.env.contains_key("MISE_YES"));
            let spec = parse("mise://core/rust@1.96.0").unwrap();
            let cached = env::load(&spec).unwrap();
            assert_eq!(cached.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            assert!(!cached.contains_key("PATH"));
            let count = std::fs::read_to_string(installs.join("mise-count")).unwrap();
            assert_eq!(count.lines().count(), 1, "{count}");
            let out2 = block_on(prepare(&config, "cargo test", dir.path(), &None)).unwrap();
            assert_eq!(out2.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            let count = std::fs::read_to_string(installs.join("mise-count")).unwrap();
            assert_eq!(count.lines().count(), 1, "{count}");
        },
    );
}

#[cfg(unix)]
#[test]
fn refresh_isolates_pin_and_drops_foreign_tool_env() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    let bin = installs.join("rust/1.96.0-iso");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&bin.join("cargo"), "echo PINNED");
    write_exec(
        &stub.join("mise"),
        r#"
while IFS= read -r line; do
  printf '%s\n' "$line"
done < "$MISE_GLOBAL_CONFIG_FILE" > "$MISE_INSTALLS_DIR/isolated.toml"
printf '%s\n' '{"RUSTUP_TOOLCHAIN":{"value":"1.96.0-iso","tool":"core:rust"},"JAVA_HOME":{"value":"/opt/java","tool":"java"},"NODE_VERSION":{"value":"24","source":"/home/u/.config/mise/config.toml"}}'
"#,
    );
    std::fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: mise://core/rust@1.96.0-iso\n",
    )
    .unwrap();
    std::fs::write(
        home.path().join("mise.toml"),
        "[tools]\njava = \"21\"\n[env]\nNODE_VERSION = \"24\"\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "cargo test", dir.path(), &None)).unwrap();
            assert_eq!(out.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0-iso");
            assert!(!out.env.contains_key("JAVA_HOME"));
            assert!(!out.env.contains_key("NODE_VERSION"));
            let isolated = std::fs::read_to_string(installs.join("isolated.toml")).unwrap();
            assert!(isolated.contains("core:rust"), "{isolated}");
            assert!(isolated.contains("1.96.0-iso"), "{isolated}");
            assert!(!isolated.contains("java"), "{isolated}");
            assert!(!isolated.contains("NODE_VERSION"), "{isolated}");
        },
    );
}

#[cfg(unix)]
#[test]
fn aqua_missing_sidecar_asks_mise_env_once() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    let bin = installs.join("jq/1.7.1");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&bin.join("jq"), "echo PINNED");
    write_exec(
        &stub.join("mise"),
        r#"printf '%s\n' '{"PATH":"/usr/bin","MISE_YES":"1"}'"#,
    );
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "jq .", dir.path(), &None)).unwrap();
            assert_eq!(out.env.len(), 1);
            assert!(out.env.contains_key("PATH"));
            let spec = parse("mise://aqua/jqlang/jq@1.7.1").unwrap();
            let cached = env::load(&spec).unwrap();
            assert!(cached.is_empty());
        },
    );
}

#[test]
fn no_pin_is_noop() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), "\"^echo\":\n  SECRET: val\n").unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let out = block_on(prepare(&config, "echo hi", dir.path(), &None)).unwrap();
        assert!(out.is_empty());
    });
}

#[cfg(unix)]
#[test]
fn install_from_url_isolates_pin_only_config() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    write_foreign_home_mise(home.path());
    let spec = parse("mise://aqua/jqlang/jq@1.7.1").unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            block_on(install::install_from_url(&spec, &installs, dir.path())).unwrap();
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(args.contains("install"), "{args}");
            assert!(!args.contains("--locked"), "{args}");
            assert_isolated_pin_only(&installs, home.path(), "1.7.1");
            assert!(installs.join("jq/1.7.1/jq").is_file());
        },
    );
}

#[cfg(unix)]
#[test]
fn install_locked_isolates_pin_only_config() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    write_foreign_home_mise(home.path());
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    let spec = parse("mise://aqua/jqlang/jq@1.7.1").unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            block_on(install::install_locked(
                "jq",
                &dir.path().join("mise.lock"),
                &spec,
                &installs,
                dir.path(),
            ))
            .unwrap();
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(args.contains("install"), "{args}");
            assert!(args.contains("--locked"), "{args}");
            assert_isolated_pin_only(&installs, home.path(), "1.7.1");
            assert!(installs.join("jq/1.7.1/jq").is_file());
        },
    );
}

#[cfg(unix)]
#[test]
fn intercept_mise_composes_project_and_pin_not_home_java() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = home.path().join("tickets");
    std::fs::create_dir_all(&tickets).unwrap();
    write_foreign_home_mise(home.path());
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nnode = \"24.16.0\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^mise:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("LADE_TICKET_DIR", Some(tickets.as_path())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "mise ls", dir.path(), &None)).unwrap();
            let composed_path = out.env.get("MISE_GLOBAL_CONFIG_FILE").unwrap();
            let composed = std::fs::read_to_string(composed_path).unwrap();
            assert!(composed.contains("node = \"24.16.0\""), "{composed}");
            assert!(composed.contains("aqua:jqlang/jq"), "{composed}");
            assert!(composed.contains("1.7.1"), "{composed}");
            assert!(!composed.contains("java"), "{composed}");
            assert!(!composed.contains("NODE_VERSION"), "{composed}");
            let ignored = out.env.get("MISE_IGNORED_CONFIG_PATHS").unwrap();
            assert!(
                ignored.contains(&home.path().join(".config/mise").display().to_string()),
                "{ignored}"
            );
            assert!(
                ignored.contains(&home.path().join("mise.toml").display().to_string()),
                "{ignored}"
            );
            assert_eq!(
                out.env.get("MISE_CONFIG_DIR").unwrap(),
                &tickets.to_string_lossy().into_owned()
            );
        },
    );
}

#[cfg(unix)]
#[test]
fn stale_sidecar_is_ignored_and_refreshed() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    let bin = installs.join("rust/1.96.0");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&bin.join("cargo"), "echo PINNED");
    temp_env::with_var("HOME", Some(home.path()), || {
        let spec = parse("mise://core/rust@1.96.0").unwrap();
        let path = env::sidecar_path(&spec);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"JAVA_HOME":"/opt/java","RUSTUP_TOOLCHAIN":"1.96.0"}"#,
        )
        .unwrap();
    });
    write_exec(
        &stub.join("mise"),
        r#"printf '%s\n' '{"RUSTUP_TOOLCHAIN":{"value":"1.96.0","tool":"core:rust"},"JAVA_HOME":{"value":"/opt/java","tool":"java"}}'"#,
    );
    std::fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "cargo test", dir.path(), &None)).unwrap();
            assert_eq!(out.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            assert!(!out.env.contains_key("JAVA_HOME"));
            let spec = parse("mise://core/rust@1.96.0").unwrap();
            let cached = env::load(&spec).unwrap();
            assert_eq!(cached.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            assert!(!cached.contains_key("JAVA_HOME"));
        },
    );
}

#[test]
fn intercept_mise_without_pins_is_noop() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), "\"^echo\":\n  SECRET: val\n").unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let out = block_on(prepare(&config, "mise ls", dir.path(), &None)).unwrap();
        assert!(out.is_empty());
        assert!(!out.env.contains_key("MISE_GLOBAL_CONFIG_FILE"));
    });
}

#[test]
fn legacy_cli_in_lade_yml_is_an_error() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: core:rust@1.96.0\n",
    )
    .unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let err = block_on(prepare(&config, "cargo test", dir.path(), &None)).unwrap_err();
        assert!(err.to_string().contains("mise://core/rust@1.96.0"), "{err}");
    });
}

#[cfg(unix)]
#[test]
fn rust_toolchain_file_does_not_override_pin() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("rust/1.96.0");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("cargo"), "echo PINNED");
    write_cached_env(
        home.path(),
        "mise://core/rust@1.96.0",
        r#"{"RUSTUP_TOOLCHAIN":"1.96.0"}"#,
    );
    std::fs::write(
        dir.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.80.0\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^cargo:\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
            ("PATH", Some(std::path::Path::new("/usr/bin"))),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "cargo test", dir.path(), &None)).unwrap();
            assert_eq!(out.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
        },
    );
}

#[cfg(unix)]
#[test]
fn lock_version_mismatch_skips_locked_install() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    write_foreign_home_mise(home.path());
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.6.0\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            block_on(prepare(&config, "jq", dir.path(), &None)).unwrap();
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(args.contains("install"), "{args}");
            assert!(!args.contains("--locked"), "{args}");
        },
    );
}

#[cfg(unix)]
#[test]
fn lock_backend_id_key_uses_locked_install() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.\"aqua:jqlang/jq\"]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            block_on(prepare(&config, "jq", dir.path(), &None)).unwrap();
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(args.contains("install"), "{args}");
            assert!(args.contains("--locked"), "{args}");
            assert!(args.contains("aqua:jqlang/jq"), "{args}");
        },
    );
}

#[cfg(unix)]
#[test]
fn refuse_when_install_leaves_bin_missing() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), "exit 0\n");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let err = block_on(prepare(&config, "jq", dir.path(), &None)).unwrap_err();
            assert!(
                err.to_string().contains("Could not run the locked"),
                "{err}"
            );
            assert!(err.to_string().contains("Homebrew"), "{err}");
        },
    );
}

#[cfg(unix)]
#[test]
fn missing_mise_is_an_error() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let empty = dir.path().join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(empty.to_str().unwrap())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let err = block_on(prepare(&config, "jq", dir.path(), &None)).unwrap_err();
            assert!(err.to_string().contains("Could not run mise"), "{err}");
        },
    );
}

#[cfg(unix)]
#[test]
fn prepare_finds_mise_bins_layout() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bins = installs.join("jq/1.7.1/.mise-bins");
    std::fs::create_dir_all(&bins).unwrap();
    write_exec(&bins.join("jq"), "echo PINNED");
    write_cached_env(home.path(), "mise://aqua/jqlang/jq@1.7.1", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
            ("PATH", Some(std::path::Path::new("/usr/bin"))),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "jq", dir.path(), &None)).unwrap();
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bins.display())), "{path}");
        },
    );
}

#[cfg(unix)]
#[test]
fn parent_lock_triggers_locked_install() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let child = dir.path().join("apps/web");
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            block_on(prepare(&config, "jq", &child, &None)).unwrap();
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(args.contains("--locked"), "{args}");
        },
    );
}

#[cfg(unix)]
#[test]
fn child_lock_without_jq_skips_locked_install() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let child = dir.path().join("apps/web");
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    std::fs::write(
        child.join("mise.lock"),
        "[[tools.node]]\nversion = \"24.16.0\"\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            block_on(prepare(&config, "jq", &child, &None)).unwrap();
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(args.contains("install"), "{args}");
            assert!(!args.contains("--locked"), "{args}");
        },
    );
}

#[test]
fn parent_mise_toml_conflict_refuses() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let child = dir.path().join("apps/web");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("mise.toml"), "[tools]\njq = \"1.6.0\"\n").unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let err = block_on(prepare(&config, "jq", &child, &None)).unwrap_err();
        assert!(err.to_string().contains("different versions"), "{err}");
    });
}
