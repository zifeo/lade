use super::*;

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
