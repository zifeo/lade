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

#[cfg(unix)]
#[test]
fn catch_all_kubectl_pin_applies_to_which() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("kubectl/1.37.0");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("kubectl"), "echo PINNED");
    write_cached_env(home.path(), "mise://aqua/kubernetes/kubectl@1.37.0", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  kubectl: mise://aqua/kubernetes/kubectl@1.37.0\n",
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
                "which kubectl",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
        },
    );
}

#[cfg(unix)]
#[test]
fn catch_all_skips_missing_sibling_pin() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("kubectl/1.37.0");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("kubectl"), "echo PINNED");
    write_cached_env(home.path(), "mise://aqua/kubernetes/kubectl@1.37.0", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  fish: mise://aqua/fish-shell/fish-shell@4.9.3\n  kubectl: mise://aqua/kubernetes/kubectl@1.37.0\n",
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
                "which kubectl",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
        },
    );
}

#[cfg(unix)]
#[test]
fn catch_all_shell_pin_stays_off_which() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let zsh_bin = installs.join("zsh/6.1.1");
    let kubectl_bin = installs.join("kubectl/1.37.0");
    std::fs::create_dir_all(&zsh_bin).unwrap();
    std::fs::create_dir_all(&kubectl_bin).unwrap();
    write_exec(&zsh_bin.join("zsh"), "echo ZSH");
    write_exec(&kubectl_bin.join("kubectl"), "echo PINNED");
    write_cached_env(home.path(), "mise://github/romkatv/zsh-bin@6.1.1", "{}");
    write_cached_env(home.path(), "mise://aqua/kubernetes/kubectl@1.37.0", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  zsh: mise://github/romkatv/zsh-bin@6.1.1\n  kubectl: mise://aqua/kubernetes/kubectl@1.37.0\n",
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
            let which = block_on(prepare(
                &config,
                "which zsh",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            let which_path = which.env.get("PATH").unwrap();
            assert!(
                which_path.starts_with(&format!("{}:", kubectl_bin.display())),
                "{which_path}"
            );
            assert!(
                !which_path.contains(&zsh_bin.display().to_string()),
                "{which_path}"
            );
            let zsh = block_on(prepare(
                &config,
                "zsh -c echo",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            let zsh_path = zsh.env.get("PATH").unwrap();
            assert!(
                zsh_path.contains(&zsh_bin.display().to_string()),
                "{zsh_path}"
            );
        },
    );
}

#[cfg(unix)]
#[test]
fn nested_pkg_fish_pin_applies_to_fish() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("aqua-fish-shell-fish-shell/4.9.3/fish.pkg/Payload/usr/local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("fish"), "echo FISH");
    write_cached_env(home.path(), "mise://aqua/fish-shell/fish-shell@4.9.3", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  fish: mise://aqua/fish-shell/fish-shell@4.9.3\n",
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
            let which = block_on(prepare(
                &config,
                "which fish",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            assert!(
                !which
                    .env
                    .get("PATH")
                    .is_some_and(|path| path.contains(&bin.display().to_string())),
                "{which:?}"
            );
            let out = block_on(prepare(
                &config,
                "fish -c echo",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
        },
    );
}

#[cfg(unix)]
#[test]
fn command_rule_kubectl_pin_skips_which() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("kubectl/1.37.0");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("kubectl"), "echo PINNED");
    write_cached_env(home.path(), "mise://aqua/kubernetes/kubectl@1.37.0", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^kubectl:\n  kubectl: mise://aqua/kubernetes/kubectl@1.37.0\n",
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
            let which = block_on(prepare(
                &config,
                "which kubectl",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            assert!(which.is_empty(), "{which:?}");
            let kubectl = block_on(prepare(
                &config,
                "kubectl get pods",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            let path = kubectl.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
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
