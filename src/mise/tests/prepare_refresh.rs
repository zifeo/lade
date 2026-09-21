use super::*;

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
            let out = block_on(prepare(
                &config,
                "cargo test",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            assert_eq!(out.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            assert_eq!(out.env.get("CARGO_HOME").unwrap(), "/c");
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
        },
    );
}

#[cfg(unix)]
#[test]
fn catch_all_rule_pins_every_matched_command() {
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
            let cargo = block_on(prepare(
                &config,
                "cargo test",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            assert_eq!(cargo.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            let which = block_on(prepare(
                &config,
                "which cargo",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            let path = which.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
            assert_eq!(which.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
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
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
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
            let out = block_on(prepare(
                &config,
                "cargo test",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            assert_eq!(out.env.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            assert!(!out.env.contains_key("MISE_YES"));
            let spec = parse("mise://core/rust@1.96.0").unwrap();
            let cached = env::load(&spec).unwrap();
            assert_eq!(cached.get("RUSTUP_TOOLCHAIN").unwrap(), "1.96.0");
            assert!(!cached.contains_key("PATH"));
            let count = std::fs::read_to_string(installs.join("mise-count")).unwrap();
            assert_eq!(count.lines().count(), 1, "{count}");
            let out2 = block_on(prepare(
                &config,
                "cargo test",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
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
            let out = block_on(prepare(
                &config,
                "cargo test",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
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
            let out =
                block_on(prepare(&config, "jq .", dir.path(), &None, Audience::Human)).unwrap();
            assert_eq!(out.env.len(), 1);
            assert!(out.env.contains_key("PATH"));
            let spec = parse("mise://aqua/jqlang/jq@1.7.1").unwrap();
            let cached = env::load(&spec).unwrap();
            assert!(cached.is_empty());
        },
    );
}
