use super::*;

#[cfg(unix)]
#[test]
fn implied_op_prepends_store() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("op/2.31.1");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("op"), "echo OP");
    write_cached_env(home.path(), "mise://aqua/1password/cli@2.31.1", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^terraform:\n  TF_VAR_FOO: op://v/i/f\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.op]]\nversion = \"2.31.1\"\nbackend = \"aqua:1password/cli\"\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(
                &config,
                "terraform plan",
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
fn yaml_pin_still_applies_when_sources_need_the_cli() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("op/2.31.1");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("op"), "echo OP");
    write_cached_env(home.path(), "mise://aqua/1password/cli@2.31.1", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  op: mise://aqua/1password/cli@2.31.1\n^terraform:\n  TF_VAR_FOO: op://v/i/f\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.op]]\nversion = \"2.31.1\"\nbackend = \"aqua:1password/cli\"\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(
                &config,
                "terraform plan",
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

#[test]
fn implied_missing_when_command_is_the_cli_refuses() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("empty-installs");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^kubectl:\n  CLUSTER: kubectl://bad-host/dev/service/postgres/5432\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let err = block_on(prepare(
                &config,
                "kubectl get pods",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap_err();
            let text = err.to_string();
            assert!(text.contains("locked kubectl is missing"), "{text}");
            assert!(text.contains("PATH binary is not used"), "{text}");
        },
    );
}

#[cfg(unix)]
#[test]
fn implied_pin_when_command_is_the_cli() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("kubectl/1.31.4");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("kubectl"), "echo KUBECTL");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^kubectl:\n  CLUSTER: kubectl://bad-host/dev/service/postgres/5432\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.kubectl]]\nversion = \"1.31.4\"\nbackend = \"aqua:kubernetes/kubectl\"\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(
                &config,
                "kubectl get pods",
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

#[test]
fn implied_missing_refuses_path_fallback() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("empty-installs");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^terraform:\n  TF_VAR_FOO: op://v/i/f\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let err = block_on(prepare(
                &config,
                "terraform plan",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap_err();
            let text = err.to_string();
            assert!(text.contains("locked op is missing"), "{text}");
            assert!(text.contains("lade setup"), "{text}");
            assert!(text.contains("PATH binary is not used"), "{text}");
        },
    );
}

#[cfg(unix)]
#[test]
fn implied_awssm_prepends_aws_bin() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("aws/2.22.35");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("aws"), "echo AWS");
    write_cached_env(home.path(), "mise://aqua/aws/aws-cli@2.22.35", "{}");
    std::fs::write(
        dir.path().join("lade.yml"),
        "^terraform:\n  SECRET: awssm://us-east-1/app/db\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.aws]]\nversion = \"2.22.35\"\nbackend = \"aqua:aws/aws-cli\"\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path())),
            ("MISE_INSTALLS_DIR", Some(installs.as_path())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let out = block_on(prepare(
                &config,
                "terraform plan",
                dir.path(),
                &None,
                Audience::Human,
            ))
            .unwrap();
            let path = out.env.get("PATH").unwrap();
            assert!(path.starts_with(&format!("{}:", bin.display())), "{path}");
            let found = locked_cli_bin("aws").expect("aws by CLI name");
            assert_eq!(found, bin.join("aws"));
            assert_eq!(locked_cli_bin("awssm").as_deref(), Some(found.as_path()));
        },
    );
}
