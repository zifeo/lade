use super::*;

#[cfg(unix)]
#[test]
fn setup_extends_mise_toml_and_lock() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    git_init(dir.path());
    std::fs::write(
        dir.path().join("mise.toml"),
        "[env]\nFOO = \"bar\"\n[tools]\nnode = \"24\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(setup_pins(PinMode::Locked));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            assert!(!dir.path().join("lade.lock").exists());
            let lock = std::fs::read_to_string(dir.path().join("mise.lock")).unwrap();
            assert!(
                lock.contains("[[tools.jq]]") || lock.contains("jqlang/jq"),
                "{lock}"
            );
            let toml = std::fs::read_to_string(dir.path().join("mise.toml")).unwrap();
            assert!(toml.contains("jq = \"1.7.1\""), "{toml}");
            assert!(toml.contains("node = \"24\""), "{toml}");
            assert!(toml.contains("FOO = \"bar\""), "{toml}");
        },
    );
}

#[cfg(unix)]
#[test]
fn setup_rewrites_bin_alias_to_backend_id() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    git_init(dir.path());
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nnode = \"24\"\nop = \"2.30.0\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^terraform:\n  TF_VAR_FOO: op://v/i/f\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(setup_pins(PinMode::Locked));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            let toml = std::fs::read_to_string(dir.path().join("mise.toml")).unwrap();
            assert!(toml.contains("node = \"24\""), "{toml}");
            assert!(
                toml.contains("\"aqua:1password/cli\" = \"2.30.0\""),
                "{toml}"
            );
            assert!(!toml.contains("op = "), "{toml}");
            let lock = std::fs::read_to_string(dir.path().join("mise.lock")).unwrap();
            assert!(lock.contains("aqua:1password/cli"), "{lock}");
            assert!(!lock.contains("tools.op"), "{lock}");
        },
    );
}

#[cfg(unix)]
#[test]
fn setup_toml_in_range_heals_stale_lock() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    std::fs::create_dir_all(installs.join("jq/1.8.0")).unwrap();
    write_exec(&installs.join("jq/1.8.0/jq"), "echo TOML");
    write_exec(&stub.join("mise"), isolation_record_stub());
    git_init(dir.path());
    std::fs::write(dir.path().join("mise.toml"), "[tools]\njq = \"1.8.0\"\n").unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@>=1.7.0\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(setup_pins(PinMode::Locked));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            let lock = std::fs::read_to_string(dir.path().join("mise.lock")).unwrap();
            assert!(lock.contains("1.8.0"), "{lock}");
            assert!(!lock.contains("1.7.1"), "{lock}");
            let toml = std::fs::read_to_string(dir.path().join("mise.toml")).unwrap();
            assert!(toml.contains("1.8.0"), "{toml}");
        },
    );
}

#[cfg(unix)]
#[test]
fn setup_exact_pin_overwrites_toml() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    git_init(dir.path());
    std::fs::write(dir.path().join("mise.toml"), "[tools]\njq = \"1.6.0\"\n").unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(setup_pins(PinMode::Locked));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            let toml = std::fs::read_to_string(dir.path().join("mise.toml")).unwrap();
            assert!(toml.contains("jq = \"1.7.1\""), "{toml}");
            assert!(!toml.contains("1.6.0"), "{toml}");
        },
    );
}
