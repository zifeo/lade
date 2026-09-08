use super::*;

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
