use super::*;

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
            let out =
                block_on(prepare(&config, "jq .", dir.path(), &None, Audience::Human)).unwrap();
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
            assert!(out.cleanup.is_empty());
            assert!(!installs.join("mise-ran").exists());
        },
    );
}

#[cfg(unix)]
#[test]
fn store_hit_without_sidecar_or_mise_still_runs() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let empty = dir.path().join("empty-path");
    let bin = installs.join("jq/1.7.1");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&empty).unwrap();
    write_exec(&bin.join("jq"), "echo PINNED");
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
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(empty.to_str().unwrap())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out =
                block_on(prepare(&config, "jq .", dir.path(), &None, Audience::Human)).unwrap();
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
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
        let err = block_on(prepare(&config, "jq", dir.path(), &None, Audience::Human)).unwrap_err();
        assert!(err.to_string().contains("is not a pin"), "{err}");
    });
}

#[test]
#[cfg(unix)]
fn project_mise_toml_is_ignored() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    std::fs::create_dir_all(installs.join("jq/1.7.1")).unwrap();
    write_exec(&installs.join("jq/1.7.1/jq"), "echo PINNED");
    write_cached_env(home.path(), "mise://aqua/jqlang/jq@1.7.1", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("mise.toml"), "[tools]\njq = \"1.6.0\"\n").unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(&config, "jq", dir.path(), &None, Audience::Human)).unwrap();
            assert!(out.env.contains_key("PATH"), "{out:?}");
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
        let out = block_on(prepare(
            &config,
            "echo hi",
            dir.path(),
            &None,
            Audience::Human,
        ))
        .unwrap();
        assert!(out.is_empty());
    });
}

#[test]
fn command_package_is_refused() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^ls:\n  NOTE: apm://github/destructure-command-hook\n",
    )
    .unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let err = block_on(prepare(&config, "ls", dir.path(), &None, Audience::Human)).unwrap_err();
        assert!(err.to_string().contains("setup package"), "{err}");
    });
}
